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
    const mode = chatViewportModeAfterScroll("pinned", { scrollHeight: 1000, clientHeight: 400, scrollTop: 600 }, {
      scrollHeight: 900, clientHeight: 400, scrollTop,
    });
    const anchor = mode === "detached" ? "Screenshot" : null;
    const delivered = [...earlier, item("Received17"), item("CQBT6P-END")];
    expect(chatWindowProjection(delivered, anchor).items).toEqual(delivered);
    expect(mode).toBe("pinned");
  });

  it("preserves the live window across composer resize, optimistic replacement, resync and stream growth", () => {
    let mode: "pinned" | "detached" = "pinned";
    let previous = { scrollHeight: 1000, clientHeight: 400, scrollTop: 600 };
    const sequence = [
      { scrollHeight: 1000, clientHeight: 450, scrollTop: 550 },
      { scrollHeight: 1050, clientHeight: 400, scrollTop: 650 },
      { scrollHeight: 1020, clientHeight: 400, scrollTop: 620 },
      { scrollHeight: 1020, clientHeight: 400, scrollTop: 619.75 },
      { scrollHeight: 1200, clientHeight: 400, scrollTop: 800 },
    ];
    for (const geometry of sequence) {
      mode = chatViewportModeAfterScroll(mode, previous, geometry);
      previous = geometry;
      expect(mode).toBe("pinned");
      expect(chatWindowProjection([item("sent"), item("reply")], mode === "pinned" ? null : "sent").items).toHaveLength(2);
    }
    expect(chatViewportModeAfterScroll(mode, previous, {
      scrollHeight: 1200, clientHeight: 400, scrollTop: 700,
    })).toBe("detached");
  });

  it("does not reattach an anchored reader when layout moves its bottom upward", () => {
    expect(chatViewportModeAfterScroll("detached", { scrollHeight: 1000, clientHeight: 400, scrollTop: 600 }, {
      scrollHeight: 900, clientHeight: 400, scrollTop: 500,
    })).toBe("detached");
  });

  it("keeps intentional history reading anchored while turns arrive", () => {
    const mode = chatViewportModeAfterScroll("pinned", { scrollHeight: 1000, clientHeight: 400, scrollTop: 600 }, {
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


describe("layout scroll before resize delivery", () => {
  it("keeps the live window through the recorded working transition and subsequent growth", () => {
    let previous = { scrollTop: 9058, scrollHeight: 9775, clientHeight: 717 };
    let mode: "pinned" | "detached" = "pinned";
    const sequence = [
      { scrollTop: 9032, scrollHeight: 9783, clientHeight: 717 },
      { scrollTop: 9032, scrollHeight: 9783, clientHeight: 699 },
      { scrollTop: 9144, scrollHeight: 9861, clientHeight: 717 },
      { scrollTop: 9236, scrollHeight: 12387, clientHeight: 717 },
    ];
    for (const viewport of sequence) {
      mode = chatViewportModeAfterScroll(mode, previous, viewport);
      expect(mode).toBe("pinned");
      expect(chatWindowProjection([item("sent"), item("reply")], mode === "pinned" ? null : "sent").items).toHaveLength(2);
      previous = { ...viewport, scrollTop: viewport.scrollHeight - viewport.clientHeight };
    }
    expect(chatViewportModeAfterScroll(mode, previous, { ...previous, scrollTop: previous.scrollTop - 26 })).toBe("detached");
  });

  it("preserves a detached reader across the same layout shift", () => {
    expect(chatViewportModeAfterScroll("detached",
      { scrollTop: 9058, scrollHeight: 9775, clientHeight: 717 },
      { scrollTop: 9032, scrollHeight: 9783, clientHeight: 717 },
    )).toBe("detached");
  });
});
