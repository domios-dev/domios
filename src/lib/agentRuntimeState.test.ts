import { describe, expect, it } from "vitest";
import {
  beginRuntimeRetry, runtimeBannerView, runtimeRetryFailed, runtimeStateFromEvent, runtimeStateFromSnapshot, runtimeViewFromEvent,
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

  it.each(["loading", "unloaded", "degraded"] as const)("shows the shared banner for %s", (state) => {
    expect(runtimeBannerView({ state, epoch: 3, reason: "opaque-fault" })).toMatchObject({
      loading: state === "loading", details: "opaque-fault", canRetry: state !== "loading",
    });
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
    expect(runtimeBannerView(loading)).toEqual({ loading: true, reason: "Loading the agent runtime.", details: "pipe closed", canRetry: false });
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

it("retains durable questions across runtime exit and rejects stale or foreign question changes", () => {
  const questions = [{ threadId: "thread", itemId: "item", response: "message" as const, createdAt: 1, questions: [] }];
  const current = { state: { state: "loaded" as const, epoch: 2 }, questions };
  const event = { tab_id: "owned", session_binding_nonce: "binding", state: { state: "unloaded" as const, epoch: 2, reason: "pipe closed" } };
  expect(runtimeViewFromEvent(current, authority, event).questions).toBe(questions);
  expect(runtimeViewFromEvent(current, authority, { ...event, tab_id: "foreign", questions: [] })).toBe(current);
  expect(runtimeViewFromEvent(current, authority, { ...event, state: { state: "degraded", epoch: 1 }, questions: [] })).toBe(current);
  expect(runtimeViewFromEvent(current, authority, { ...event, questions: [] }).questions).toEqual([]);
});

describe("runtime banner failure reasons", () => {
  const rawDetail = 'Exception { stack: "at <anonymous> (eval_script:16:66)" }';
  const cases = [
    ["SizeLimit", "Runtime hook state size limit", "The agent runtime exceeded its size limit."],
    ["SpawnFailed", "Runtime could not start:", "The agent runtime could not start."],
    ["ProcessExited", "Runtime exited (code 1):", "The agent runtime stopped."],
    ["PipeEof", "Runtime connection closed", "The connection to the agent runtime closed."],
    ["PipeFailure", "Runtime stdout connection failed:", "The connection to the agent runtime failed."],
    ["InvalidFrame", "Runtime sent an invalid response:", "The agent runtime sent an invalid response."],
    ["Deadline", "Runtime did not respond in time", "The agent runtime did not respond in time."],
    ["HookFailed", "Provider operation failed:", "The provider plugin failed while handling a response."],
    ["Refused", "Provider refused:", "The provider refused the operation."],
    ["PrewriteRefused", "Input was not sent:", "The input could not be sent to the agent runtime."],
    ["Superseded", "Runtime session changed; this operation was cancelled", "The agent session changed and the operation was canceled."],
  ];

  it.each(cases)("maps %s to a human reason and preserves diagnostics separately", (code, display, expected) => {
    for (const reason of [`${code}: ${rawDetail}`, `${display} ${rawDetail}`]) {
      expect(runtimeBannerView({ state: "degraded", epoch: 1, reason })).toEqual({
        loading: false, reason: expected, details: reason, canRetry: true,
      });
    }
  });

  it("recognizes the nested runtime size limit from a provider hook exception", () => {
    const reason = `Provider operation failed: HookFailed: runtimeResponse: Exception { message: Some("runtime size limit"), stack: ${rawDetail} }`;
    expect(runtimeBannerView({ state: "unloaded", epoch: 1, reason })?.reason)
      .toBe("The agent runtime exceeded its size limit.");
  });

  it.each([null, undefined, "", "   ", rawDetail])("keeps unknown or absent faults out of the human reason: %s", (reason) => {
    expect(runtimeBannerView({ state: "degraded", epoch: 1, reason })).toEqual({
      loading: false, reason: "The agent runtime is unavailable.", details: reason?.trim() ? reason : null, canRetry: true,
    });
  });
});
