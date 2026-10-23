import { describe, expect, it } from "vitest";
import { chatLayoutScrollTarget, shouldCaptureChatAnchor } from "./chatScrollDecisions";
import { chatViewportModeAfterScroll } from "./chatViewport";
import { captureChatAnchor, chatAnchorIdentities, chatRevealIndex, resolveChatAnchorIndex, resolveChatAnchorTop } from "./chatAnchorModel";
import { captureChatContentAnchor } from "./chatScrollAnchor";

describe("chat anchor identity and pixels", () => {
  it("measures the anchor offset from the message body when a day separator precedes it", () => {
    const scroll = document.createElement("div");
    scroll.scrollTop = 500;
    const frame = document.createElement("div");
    frame.dataset.chatWindowRow = "opaque-row";
    const message = document.createElement("article");
    message.dataset.messageId = "opaque-row";
    frame.append(message);
    scroll.append(frame);
    scroll.getBoundingClientRect = () => ({ top: 100 } as DOMRect);
    frame.getBoundingClientRect = () => ({ top: 50, height: 1000 } as DOMRect);
    message.getBoundingClientRect = () => ({ top: 80, height: 970 } as DOMRect);
    expect(captureChatContentAnchor(scroll)).toEqual({ id: "opaque-row", key: "opaque-row", ordinal: 0, offset: -20 });
  });
  it("keeps the anchor body still when its day separator moves onto a prepended page", () => {
    const separator = 28;
    const [first, second] = chatAnchorIdentities([{ id: "first" }, { id: "second" }]);
    const before = [
      { ...first, top: 0, height: separator + 50, contentTop: separator },
      { ...second, top: separator + 50, height: 50, contentTop: separator + 50 },
    ];
    const scrollTop = 10;
    const anchor = captureChatAnchor(before, scrollTop)!;
    const prepended = 400;
    const after = [
      { ...first, top: prepended, height: 50, contentTop: prepended },
      { ...second, top: prepended + 50, height: 50, contentTop: prepended + 50 },
    ];
    const nextScrollTop = resolveChatAnchorTop(after, anchor)!;
    for (const [index, row] of after.entries()) {
      expect(row.contentTop - nextScrollTop).toBe(before[index].contentTop - scrollTop);
    }
  });
  const identities = chatAnchorIdentities([{ id: "range-a", sourceMessageId: "duplicate" }, { id: "range-b", sourceMessageId: "duplicate" }, { id: "range-c" }]);
  it("captures the first intersecting row with its viewport offset", () => {
    expect(captureChatAnchor(identities.map((row, index) => ({ ...row, top: index * 100 })), 80))
      .toEqual({ id: "range-a", key: "duplicate", ordinal: 0, offset: -80 });
  });
  it("resolves stable render identity before a reused source id", () => {
    const anchor = { id: "range-b", key: "duplicate", ordinal: 1, offset: 20 };
    expect(resolveChatAnchorIndex(identities.slice(1), anchor)).toBe(0);
  });
  it("keeps global occurrence identity when only a mounted slice is measured", () => {
    const anchor = { id: "old-b", key: "duplicate", ordinal: 1, offset: 20 };
    expect(resolveChatAnchorTop([{ ...identities[1], top: 570 }], anchor)).toBe(550);
    expect(resolveChatAnchorTop([{ ...identities[0], top: 570 }], anchor)).toBeNull();
  });
  it("resolves source reveal identity before a colliding render key", () => {
    const messages = [{ id: "target", sourceMessageId: "decoy" }, { id: "other", sourceMessageId: "target" }];
    expect(chatRevealIndex(messages, "target")).toBe(1);
    expect(chatRevealIndex(messages, "other")).toBe(1);
    expect(chatRevealIndex(messages, "missing")).toBe(-1);
  });
  it("preserves the pixel offset after prepend and variable height growth", () => {
    const anchor = { id: "range-b", key: "duplicate", ordinal: 1, offset: -35 };
    expect(resolveChatAnchorTop([{ ...identities[1], top: 950 }], anchor)).toBe(985);
    expect(captureChatAnchor([], 0)).toBeNull();
  });
});


describe("chat layout ownership", () => {
  const before = { scrollTop: 31226, scrollHeight: 31876, clientHeight: 650 };
  it("repairs pinned inset growth without a transcript change", () => {
    const after = { ...before, scrollHeight: before.scrollHeight + 42 };
    expect(chatLayoutScrollTarget("pinned", after, null)).toBe(31918);
    expect(chatViewportModeAfterScroll("pinned", before, after)).toBe("pinned");
  });
  it("repairs a phone composer resize without detaching", () => {
    const after = { ...before, clientHeight: 572 };
    expect(chatViewportModeAfterScroll("pinned", before, after)).toBe("pinned");
    expect(chatLayoutScrollTarget("pinned", after, null)).toBe(31876);
  });
  it("preserves a detached anchor after a mounted extent clamp", () => {
    const detached = { scrollTop: 127103, scrollHeight: 128053, clientHeight: 650 };
    const clamped = { scrollTop: 121033, scrollHeight: 121683, clientHeight: 650 };
    expect(shouldCaptureChatAnchor(detached, clamped)).toBe(false);
    expect(chatViewportModeAfterScroll("detached", detached, clamped)).toBe("detached");
    expect(chatLayoutScrollTarget("detached", detached, 127103)).toBe(127103);
  });
  it("distinguishes an unhandled reader movement from layout repair", () => {
    expect(shouldCaptureChatAnchor(before, { ...before, scrollTop: before.scrollTop - 300 })).toBe(true);
    expect(chatViewportModeAfterScroll("pinned", before, { ...before, scrollTop: before.scrollTop - 300 })).toBe("detached");
    expect(chatViewportModeAfterScroll("detached", { ...before, scrollTop: before.scrollTop - 300 }, before)).toBe("pinned");
  });
});
