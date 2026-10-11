import { describe, expect, it } from "vitest";
import { captureChatAnchor, chatAnchorIdentities, chatRevealIndex, resolveChatAnchorIndex, resolveChatAnchorTop } from "./chatAnchorModel";

describe("chat anchor identity and pixels", () => {
  const identities = chatAnchorIdentities([{ id: "range-a", sourceMessageId: "duplicate" }, { id: "range-b", sourceMessageId: "duplicate" }, { id: "range-c" }]);
  it("captures the first intact row with its viewport offset", () => {
    expect(captureChatAnchor(identities.map((row, index) => ({ ...row, top: index * 100 })), 80))
      .toEqual({ id: "range-b", key: "duplicate", ordinal: 1, offset: 20 });
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
