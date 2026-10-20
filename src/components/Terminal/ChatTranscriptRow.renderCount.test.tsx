import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { EnrichedChatEntry } from "./chatEnrichment";
import { ChatTranscriptRow, type ChatTranscriptRowContext, type OutboundReportEntry } from "./ChatTranscriptRow";
import type { MessageRowHandlers } from "./ChatMessageRow";

const bubbleRenders = new Map<string, number>();

vi.mock("@codeterm/chat-ui", async (original) => {
  const actual = await original<typeof import("@codeterm/chat-ui")>();
  return {
    ...actual,
    MessageBubble: ({ message }: { message: NormalizedChatMessage }) => {
      bubbleRenders.set(message.id, (bubbleRenders.get(message.id) ?? 0) + 1);
      return <p>{message.content}</p>;
    },
  };
});

const handlers: MessageRowHandlers = {
  clipFor: () => null, partsFor: () => [], selectedFor: () => false, cursorFor: () => false, joinNextFor: () => false,
  select: () => {}, rowMouseDown: () => {}, toggle: () => {}, range: () => {}, endRange: () => {}, quote: () => {}, ask: () => {},
};

const context: ChatTranscriptRowContext = {
  tabId: "tab", promptAnswers: true, answeredInteractiveIds: new Set(), handlers,
  onContentLayout: () => {}, resolveTabName: () => undefined, resolvePeerName: () => undefined, onOpenPlan: () => {},
  resolvePlanRef: () => null, onRerunShellCommand: () => {}, onStopReminder: () => {}, onOpenJobs: () => {}, onFocusTab: () => {},
  onJumpToMessage: () => {}, onAnswerAsk: () => {}, deleteSend: () => {}, retrySend: () => {}, forkFrom: () => {}, revertTo: () => {},
};

function entry(id: string, content: string): EnrichedChatEntry {
  const msg = { id, type: "assistant", content } as NormalizedChatMessage;
  return { msg, item: { kind: "message", message: msg }, origin: undefined };
}

const report: OutboundReportEntry = { targetTabId: "parent", at: null, descriptor: {
  reportId: "r1", recipient: "Parent", task: null, status: "in_review", sha: null, message: "Ready for review", taskOutcome: null,
} };

function Transcript({ entries, reportsAfter }: { entries: EnrichedChatEntry[]; reportsAfter: Record<string, OutboundReportEntry[]> }) {
  return <>{entries.map((row) => <ChatTranscriptRow key={row.msg.id} entry={row} reports={reportsAfter[row.msg.id] ?? []} asks={[]}
    ariaLabel={row.msg.content} selected={false} joinNext={false} selecting={false} cursor={false} streaming={false}
    latestFailedToolCall={false} forkable={false} revertible={false} context={context} />)}</>;
}

describe("transcript rows", () => {
  let host: HTMLDivElement;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    bubbleRenders.clear();
    host = document.createElement("div");
    document.body.append(host);
  });
  afterEach(() => host.remove());

  it("re-render only the row whose message changed while the transcript and its rails are rebuilt", async () => {
    const root = createRoot(host);
    const rows = [entry("a", "first"), entry("b", "second"), entry("c", "third")];
    await act(async () => root.render(<Transcript entries={rows} reportsAfter={{ a: [report] }} />));
    expect(Object.fromEntries(bubbleRenders)).toEqual({ a: 1, b: 1, c: 1 });

    const streamed = [rows[0], entry("b", "second, streamed"), rows[2]];
    await act(async () => root.render(<Transcript entries={streamed} reportsAfter={{ a: [report] }} />));
    expect(Object.fromEntries(bubbleRenders)).toEqual({ a: 1, b: 2, c: 1 });
    expect(host.textContent).toContain("second, streamed");
    expect(host.querySelector('[data-report-id="r1"]')).not.toBeNull();
    await act(async () => root.unmount());
  });
});
