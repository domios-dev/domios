import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import type { NormalizedChatMessage, TimelineItem } from "@codeterm/chat-core";
import type { MessageRowHandlers } from "./ChatMessageRow";
import { useChatMessageSelection, type SelectableRow } from "./useChatMessageSelection";

function row(id: string, content: string): SelectableRow {
  const msg = { id, type: "assistant", content } as NormalizedChatMessage;
  return { msg, item: { kind: "message", message: msg } as TimelineItem };
}

describe("message selection row handlers", () => {
  it("keep their identity while streamed rows change and still read the latest parts", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const seen: MessageRowHandlers[] = [];
    function Harness({ rows }: { rows: SelectableRow[] }) {
      const surfaceRef = useRef<HTMLDivElement>(null);
      seen.push(useChatMessageSelection({ tabId: "tab", rows, agent: "Agent", surfaceRef }).rowHandlers);
      return <div ref={surfaceRef} />;
    }
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Harness rows={[row("a", "one")]} />));
    await act(async () => root.render(<Harness rows={[row("a", "one, then more"), row("b", "two")]} />));
    const [first, last] = [seen[0], seen[seen.length - 1]];
    expect(last).toBe(first);
    expect(last.partsFor("b").length).toBeGreaterThan(0);
    await act(async () => root.unmount());
  });
});
