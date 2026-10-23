import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { useChatSession } from "./useChatSession";
import { clearTranscriptWindows } from "./transcriptStore";
import { chatBodyView } from "./chatBodyVisibility";
import { ChatSetupState } from "./ChatSetupState";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let host: HTMLDivElement;
let root: Root;
let starts: number;
let failSnapshot: boolean;
let failListener: boolean;
const authority = { source_session_id: "own-session", session_binding_nonce: "own-nonce" };

function Harness({ active = true }: { active?: boolean }) {
  const session = useChatSession({ tabId: "failed-attach", agentSessionId: "own-session",
    sessionBindingNonce: "own-nonce", active, retainAttachment: true });
  const view = chatBodyView({ snapshotReady: session.snapshotReady, hasTranscript: session.allMessages.length > 0,
    chatLoading: session.loading, attachmentError: session.attachmentState === "failed" ? session.error : null,
    unavailable: session.transcriptUnavailable, bootView: { kind: "none" } });
  return <><ChatSetupState tab={null} providerName="Fixture" view={view} onRetry={session.retry} />
    {view.kind === "none" && <div role="log">{session.allMessages.map(row => row.content).join("")}</div>}</>;
}

beforeEach(() => {
  clearTranscriptWindows();
  starts = 0;
  failSnapshot = true;
  failListener = false;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  __setInvokeForTest((async (command: string, args: Record<string, unknown>) => {
    if (command !== "chat_start") return [];
    starts++;
    if (failSnapshot) throw new Error("request_busy");
    return { ...authority, subscription_id: args.subscriptionId, messages: [{ id: "row", type: "assistant", content: "Restored transcript" }],
      text: "", start_offset: 0, offset: 40, pending: false };
  }) as never);
  __setListenForTest(async event => {
    if (event === "tab-chat-msg-failed-attach" && failListener) throw new Error("listener refused");
    return () => {};
  });
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  __setInvokeForTest(null);
  __setListenForTest(null);
  clearTranscriptWindows();
});

it("shows a failed snapshot and Retry commits a fresh authoritative transcript", async () => {
  await act(async () => root.render(<Harness />));
  expect(host.querySelector('[aria-label="Chat connection failed"]')?.textContent).toContain("request_busy");
  expect(host.querySelector('[data-testid="chat-setup-state"]')).toBeNull();
  expect(starts).toBe(1);
  failSnapshot = false;
  await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
  expect(host.querySelector('[role="log"]')?.textContent).toBe("Restored transcript");
  expect(host.querySelector("button")).toBeNull();
  expect(starts).toBe(2);
});

it("retries a failed snapshot on tab lifecycle activation", async () => {
  await act(async () => root.render(<Harness />));
  failSnapshot = false;
  await act(async () => root.render(<Harness active={false} />));
  await act(async () => root.render(<Harness />));
  expect(host.querySelector('[role="log"]')?.textContent).toBe("Restored transcript");
  expect(starts).toBe(2);
});

it("Retry reinstalls a rejected event listener before requesting the snapshot", async () => {
  failListener = true;
  failSnapshot = false;
  await act(async () => root.render(<Harness />));
  expect(host.textContent).toContain("listener refused");
  expect(starts).toBe(0);
  failListener = false;
  await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
  expect(host.querySelector('[role="log"]')?.textContent).toBe("Restored transcript");
  expect(starts).toBe(1);
});

