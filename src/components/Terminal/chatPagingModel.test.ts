import { describe, expect, it } from "vitest";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { CHAT_PAGE_SIZE, conversationEntries, earlierMessageLabel, revealEarlierMessages } from "./chatPagingModel";
import { chatWindowProjection, chatWindowRange, timelineItemKey } from "./chatWindow";

const text = (index: number): NormalizedChatMessage => ({ id: `m-${index}`, type: "assistant", content: `body-${index}` });

describe("conversation paging", () => {
  it("counts split envelopes and grouped tools in their rendered units", () => {
    const messages: NormalizedChatMessage[] = [{ id: "envelopes", type: "user", content: "", segments:
      Array.from({ length: 130 }, (_, index) => ({ type: "text" as const, text: `body-${index}`, correlationId: `c-${index}` })),
    }];
    for (let index = 0; index < 200; index++) {
      messages.push({ id: `call-${index}`, type: "tool_call", content: "", toolId: `tool-${index}`,
        toolName: "exec", toolSummary: "opaque", toolDescription: "opaque", toolFlowId: "flow" });
      messages.push({ id: `result-${index}`, type: "tool_result", content: "opaque", toolId: `tool-${index}` });
    }
    messages.push({ id: "blank", type: "thinking", content: "" });
    const entries = conversationEntries(messages);
    expect(entries).toHaveLength(131);
    expect(entries[entries.length - 1]?.item.kind).toBe("tool");
    const items = entries.map(entry => entry.item);
    expect(chatWindowProjection(items, null, CHAT_PAGE_SIZE).items).toHaveLength(100);
    expect(revealEarlierMessages(items, null, CHAT_PAGE_SIZE).count).toBe(31);
  });

  it("reveals 100 each time while retaining the current top row and bounding the reading window", () => {
    const items = conversationEntries(Array.from({ length: 10_000 }, (_, index) => text(index))).map(entry => entry.item);
    let anchor: string | null = null;
    let size = CHAT_PAGE_SIZE;
    for (let click = 0; click < 20; click++) {
      const before = chatWindowRange(items, anchor, size);
      const first = timelineItemKey(items[before.start]);
      const next = revealEarlierMessages(items, anchor, size);
      const after = chatWindowRange(items, next.anchor, next.size);
      expect(before.start - after.start).toBe(100);
      expect(items.slice(after.start, after.end).some(item => timelineItemKey(item) === first)).toBe(true);
      expect(after.end - after.start).toBeLessThanOrEqual(300);
      anchor = next.anchor;
      size = next.size;
    }
  });

  it("never fills unused initial capacity with an extra unlabelled burst", () => {
    const items = conversationEntries(Array.from({ length: 105 }, (_, index) => text(index))).map(entry => entry.item);
    const next = revealEarlierMessages(items, null, 5);
    expect(next.size).toBe(105);
    expect(next.count).toBe(100);
    expect(earlierMessageLabel(3, false)).toBe("Load previous 3 messages");
    expect(earlierMessageLabel(1, false)).toBe("Load previous 1 message");
    expect(earlierMessageLabel(3, true)).toBe("Load previous messages (up to 100)");
  });
});
