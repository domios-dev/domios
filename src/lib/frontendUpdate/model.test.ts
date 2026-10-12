import { describe, expect, it } from "vitest";
import { initialSwapState, swapEvent, compatible, observeBoot, recoveryTarget, safeToSwap, type Safety, type Release } from "./model";
import type { ClientCheckpoint } from "./checkpoint";

const checkpoint: ClientCheckpoint = { activeWorkspaceId: "w", activeTabId: "a", drafts: { a: { text: "draft", attachmentIds: ["attachment-1"], replyTo: null } }, scroll: { a: { terminal: "line-42", chat: "message-8" } } };
const safe: Safety = { focused: true, idle: true, typing: false, composing: false, modalOpen: false, pendingUploads: 0 };
const release: Release = { releaseId: "frontend-v1.0.0", version: "1.0.0", minApiVersion: 2, maxApiVersion: 2 };
const ticket = { releaseId: release.releaseId, generation: 1, token: "health-token" };

describe("frontend swap safety", () => {
  it("requires known inclusive API bounds", () => {
    expect(compatible(release, 2)).toBe(true);
    for (const api of [0, 1, 3, NaN, 2.5]) expect(compatible(release, api)).toBe(false);
    expect(compatible({ ...release, minApiVersion: 0 }, 2)).toBe(false);
    expect(compatible({ ...release, maxApiVersion: 1 }, 2)).toBe(false);
  });
  it.each([{ typing: true }, { composing: true }, { modalOpen: true }, { pendingUploads: 1 }, { focused: false }, { idle: false }])("does not reload while unsafe %j", changes => {
    const state = initialSwapState(2, checkpoint, { ...safe, ...changes });
    const pending = swapEvent(state, { type: "ready", release });
    expect(pending.effects).toEqual([]);
    const focus = swapEvent(pending.state, { type: "safety", safety: { ...pending.state.safety, focused: true } });
    if (safeToSwap(focus.state.safety)) expect(focus.effects[0]?.type).toBe("persist");
    else expect(focus.effects).toEqual([]);
  });
  it("persists edits made during checkpoint and activation before reloading", () => {
    const pending = swapEvent(initialSwapState(2, checkpoint, safe), { type: "ready", release });
    const write = pending.state.writing!;
    const last = structuredClone(checkpoint);
    last.drafts.a.text = "exact final edit";
    last.drafts.a.attachmentIds.push("attachment-2");
    const edited = swapEvent(pending.state, { type: "edited", checkpoint: last });
    expect(edited.effects).toEqual([]);
    const staleAck = swapEvent(edited.state, { type: "persisted", write });
    expect(staleAck.effects).toEqual([{ type: "persist", write: { revision: 1, payload: last } }]);
    const persisted = swapEvent(staleAck.state, { type: "persisted", write: staleAck.state.writing! });
    expect(persisted.effects).toEqual([{ type: "activate", releaseId: release.releaseId }]);
    const nextEdit = swapEvent(persisted.state, { type: "edited", checkpoint: { ...last, activeTabId: "b" } });
    const activated = swapEvent(nextEdit.state, { type: "activated", ticket });
    expect(activated.effects).toEqual([]);
    const final = swapEvent(activated.state, { type: "persisted", write: activated.state.writing! });
    expect(final.effects).toEqual([{ type: "reload", ticket }]);
    expect(final.state.checkpoint.scroll.a.chat).toBe("message-8");
    expect(swapEvent(final.state, { type: "safety", safety: safe }).effects).toEqual([]);
  });
  it("rechecks modal and focus after persistence ack", () => {
    const pending = swapEvent(initialSwapState(2, checkpoint, safe), { type: "ready", release });
    const unsafe = swapEvent(pending.state, { type: "safety", safety: { ...safe, modalOpen: true } });
    const ack = swapEvent(unsafe.state, { type: "persisted", write: pending.state.writing! });
    expect(ack.effects).toEqual([]);
    const focus = swapEvent(ack.state, { type: "safety", safety: { ...safe, modalOpen: true } });
    expect(focus.effects).toEqual([]);
    expect(swapEvent(focus.state, { type: "safety", safety: safe }).effects[0]?.type).toBe("activate");
  });
  it("ignores altered checkpoint ack and foreign activation", () => {
    const pending = swapEvent(initialSwapState(2, checkpoint, safe), { type: "ready", release });
    const altered = swapEvent(pending.state, { type: "persisted", write: { ...pending.state.writing!, payload: { ...checkpoint, activeTabId: "other" } } });
    expect(altered.state.acknowledgedRevision).toBeNull();
    expect(swapEvent(altered.state, { type: "activated", ticket: { ...ticket, releaseId: "foreign" } }).state.ticket).toBeNull();
  });
  it("deadline only reports failure and stale render cannot revive it", () => {
    const boot = { ticket, deadlineMs: 100, rendered: false, failed: false };
    expect(observeBoot(boot, { type: "deadline", nowMs: 99 }).report).toBeNull();
    const failed = observeBoot(boot, { type: "deadline", nowMs: 100 });
    expect(failed.report).toBe("rollback");
    expect(observeBoot(failed.state, { type: "rootRendered", nowMs: 101 }).report).toBeNull();
    const healthy = observeBoot(boot, { type: "rootRendered", nowMs: 99 });
    expect(healthy.report).toBe("healthy");
    expect(observeBoot(healthy.state, { type: "deadline", nowMs: 100 }).report).toBeNull();
    expect(observeBoot(healthy.state, { type: "exception", nowMs: 101 }).report).toBe("rollback");
  });
  it("requires root evidence independently for each enrolled client", () => {
    const first = { ticket, deadlineMs: 100, rendered: false, failed: false };
    const second = { ...first, ticket: { ...ticket, token: "second-health-token" } };
    const rendered = observeBoot(first, { type: "rootRendered", nowMs: 99 });
    expect(rendered.report).toBe("healthy");
    expect(observeBoot(rendered.state, { type: "deadline", nowMs: 100 }).report).toBeNull();
    expect(observeBoot(second, { type: "deadline", nowMs: 100 }).report).toBe("rollback");
    expect(second.rendered).toBe(false);
  });
  it("bounds rollback chains and requires compatible recovery", () => {
    const bundled = { ...release, releaseId: "bundled" };
    expect(recoveryTarget(release, bundled, 2, [release.releaseId])).toEqual(bundled);
    expect(recoveryTarget(release, bundled, 3, [])).toBeNull();
    expect(recoveryTarget(release, bundled, 2, [release.releaseId, "bundled"])).toBeNull();
    expect(recoveryTarget(release, bundled, 2, ["a", "b", "c"])).toBeNull();
  });
});
