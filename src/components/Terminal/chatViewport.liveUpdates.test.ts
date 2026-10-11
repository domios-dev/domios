import { describe, expect, it } from "vitest";
import type { TimelineItem } from "@codeterm/chat-core";
import { chatViewportModeAfterScroll } from "./chatViewport";
import { chatWindowProjection } from "./chatWindow";

const item = (id: string): TimelineItem => ({
  kind: "message",
  message: { id, type: "assistant", content: id },
});

describe("live transcript after viewport clamping", () => {
  it.each([500, 499.75])("keeps delivered turns visible after layout clamps the bottom to %s", (scrollTop) => {
    const earlier = [item("earlier"), item("Screenshot")];
    const mode = chatViewportModeAfterScroll("pinned", 600, {
      scrollHeight: 900, clientHeight: 400, scrollTop,
    });
    const anchor = mode === "detached" ? "Screenshot" : null;
    const delivered = [...earlier, item("Received17"), item("CQBT6P-END")];
    expect(chatWindowProjection(delivered, anchor).items).toEqual(delivered);
    expect(mode).toBe("pinned");
  });

  it("does not reattach an anchored reader when layout moves its bottom upward", () => {
    expect(chatViewportModeAfterScroll("detached", 600, {
      scrollHeight: 900, clientHeight: 400, scrollTop: 500,
    })).toBe("detached");
  });

  it("keeps intentional history reading anchored while turns arrive", () => {
    const mode = chatViewportModeAfterScroll("pinned", 600, {
      scrollHeight: 1000, clientHeight: 400, scrollTop: 550,
    });
    expect(mode).toBe("detached");
    const earlier = [item("earlier"), item("Screenshot")];
    expect(chatWindowProjection([...earlier, item("new")], "Screenshot").items).toEqual(earlier);
    expect(chatWindowProjection([...earlier, item("new")], null).items).toHaveLength(3);
  });
});
