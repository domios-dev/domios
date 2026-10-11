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

  it("preserves the live window across composer resize, optimistic replacement, resync and stream growth", () => {
    let mode: "pinned" | "detached" = "pinned";
    let previousTop = 600;
    const sequence = [
      { scrollHeight: 1000, clientHeight: 450, scrollTop: 550 },
      { scrollHeight: 1050, clientHeight: 400, scrollTop: 650 },
      { scrollHeight: 1020, clientHeight: 400, scrollTop: 620 },
      { scrollHeight: 1020, clientHeight: 400, scrollTop: 619.75 },
      { scrollHeight: 1200, clientHeight: 400, scrollTop: 800 },
    ];
    for (const geometry of sequence) {
      mode = chatViewportModeAfterScroll(mode, previousTop, geometry);
      previousTop = geometry.scrollTop;
      expect(mode).toBe("pinned");
      expect(chatWindowProjection([item("sent"), item("reply")], mode === "pinned" ? null : "sent").items).toHaveLength(2);
    }
    expect(chatViewportModeAfterScroll(mode, previousTop, {
      scrollHeight: 1200, clientHeight: 400, scrollTop: 700,
    })).toBe("detached");
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
    const projection = chatWindowProjection([...earlier, item("new")], "Screenshot");
    expect(projection.items).toEqual(earlier);
    expect(projection.newerCount).toBe(1);
    expect(chatWindowProjection([...earlier, item("new")], null).newerCount).toBe(0);
    expect(chatWindowProjection([...earlier, item("new")], null).items).toHaveLength(3);
  });
});
