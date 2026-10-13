import { describe, expect, it } from "vitest";
import {
  beginRuntimeRetry, runtimeBannerView, runtimeRetryFailed, runtimeStateFromEvent, runtimeStateFromSnapshot,
  type AgentRuntimeState,
} from "./agentRuntimeState";

const authority = { tabId: "owned", nonce: "binding" };
const states: AgentRuntimeState["state"][] = ["loading", "loaded", "unloaded", "degraded"];

describe("agent runtime state", () => {
  it.each(states.flatMap((from) => states.map((to) => [from, to] as const)))
    ("accepts core transition %s → %s", (from, to) => {
      const result = runtimeStateFromEvent({ state: from, epoch: 2, reason: "old" }, authority, {
        tab_id: "owned", session_binding_nonce: "binding", state: { state: to, epoch: 3, reason: "fault" },
      });
      expect(result).toEqual(to === "loaded" ? { state: "loaded", epoch: 3 } : { state: to, epoch: 3, reason: "fault" });
    });

  it.each(states)("hydrates %s from a restored snapshot", (state) => {
    expect(runtimeStateFromSnapshot(null, { state, epoch: 1, reason: "saved" })?.state).toBe(state);
  });

  it.each(states)("rejects an old epoch's %s event", (state) => {
    const current: AgentRuntimeState = { state: "loaded", epoch: 4 };
    expect(runtimeStateFromEvent(current, authority, {
      tab_id: "owned", session_binding_nonce: "binding", state: { state, epoch: 3 },
    })).toBe(current);
  });

  it.each([["foreign", "binding"], ["owned", "prior"], ["owned", null]])
    ("rejects mismatched authority %s/%s", (tab_id, session_binding_nonce) => {
      const current: AgentRuntimeState = { state: "loaded", epoch: 1 };
      expect(runtimeStateFromEvent(current, authority, {
        tab_id: tab_id!, session_binding_nonce, state: { state: "degraded", epoch: 2, reason: "foreign" },
      })).toBe(current);
    });

  it.each(["unloaded", "degraded"] as const)("keeps the %s reason while Retry loads", (state) => {
    const starting = { state, epoch: 1, reason: "pipe closed" };
    const loading = beginRuntimeRetry(starting);
    expect(loading).toEqual({ state: "loading", epoch: 1, reason: "pipe closed" });
    expect(runtimeStateFromSnapshot(loading, { state: "loading", epoch: 2 })).toEqual({ state: "loading", epoch: 2, reason: "pipe closed" });
    expect(runtimeBannerView(loading)).toEqual({ loading: true, reason: "pipe closed", canRetry: false });
    expect(beginRuntimeRetry(loading)).toBe(loading);
    const failed = runtimeRetryFailed(loading, "resume refused");
    expect(failed).toEqual({ state: "degraded", epoch: 1, reason: "resume refused" });
    expect(runtimeBannerView(failed)?.canRetry).toBe(true);
  });

  it("only authoritative loaded state clears the failure", () => {
    const state = runtimeStateFromSnapshot({ state: "loading", epoch: 2, reason: "fault" }, { state: "loaded", epoch: 2 });
    expect(state).toEqual({ state: "loaded", epoch: 2 });
    expect(runtimeBannerView(state)).toBeNull();
    expect(beginRuntimeRetry(state)).toBe(state);
    expect(runtimeRetryFailed(state, "late rejection")).toBe(state);
  });

  it("keeps non-runtime tabs silent and reports missing fault reasons honestly", () => {
    expect(runtimeStateFromSnapshot(null, null)).toBeNull();
    expect(runtimeBannerView(null)).toBeNull();
    expect(beginRuntimeRetry(null)).toBeNull();
    expect(runtimeRetryFailed(null, "fault")).toBeNull();
    expect(runtimeStateFromSnapshot(null, { state: "degraded", epoch: 0 })?.reason).toBeTruthy();
  });
});
