import { describe, expect, it } from "vitest";
import { checkpointWrite, checkpointAcknowledged, type ClientCheckpoint } from "./checkpoint";

const payload: ClientCheckpoint = { activeWorkspaceId: "w", activeTabId: "t", drafts: { t: { text: "draft", attachmentIds: ["a"], replyTo: "m" } }, scroll: { t: { terminal: "row", chat: "message" } } };

describe("frontend checkpoint", () => {
  it("freezes a complete snapshot without mutating the composer", () => {
    const source = structuredClone(payload);
    const write = checkpointWrite(7, source);
    source.drafts.t.text = "later";
    source.drafts.t.attachmentIds.push("b");
    expect(write.payload).toEqual(payload);
    expect(checkpointAcknowledged(write, structuredClone(write))).toBe(true);
  });
  it("rejects a stale revision, missing scope and lost attachment", () => {
    const write = checkpointWrite(7, payload);
    expect(checkpointAcknowledged(write, { ...write, revision: 6 })).toBe(false);
    const lost = structuredClone(write);
    lost.payload.drafts.t.attachmentIds = [];
    expect(checkpointAcknowledged(write, lost)).toBe(false);
    delete lost.payload.scroll.t;
    expect(checkpointAcknowledged(write, lost)).toBe(false);
  });
  it("compares object keys independent of serialization order", () => {
    const write = checkpointWrite(7, payload);
    expect(checkpointAcknowledged(write, { revision: 7, payload: { scroll: payload.scroll, drafts: payload.drafts, activeTabId: "t", activeWorkspaceId: "w" } })).toBe(true);
  });
  it("refuses revisions that cannot be represented exactly", () => {
    for (const revision of [-1, NaN, 0.5, Number.MAX_SAFE_INTEGER + 1]) expect(() => checkpointWrite(revision, payload)).toThrow();
  });
});
