import { describe, expect, it } from "vitest";
import {
  beginRuntimeRetry, runtimeResponseDeadlinePending, runtimeStatusPresentation, runtimeRetryFailed, runtimeStateFromEvent, runtimeStateFromSnapshot, runtimeViewFromEvent, runtimeHistorySettled,
  type AgentRuntimeState,
} from "./agentRuntimeState";

const authority = { tabId: "owned", nonce: "binding" };
const states: AgentRuntimeState["state"][] = ["loading", "loaded", "unloaded", "degraded"];

describe("agent runtime state", () => {
  it("keeps login pending actionable without a stale launch failure", () => {
    const state = runtimeStateFromSnapshot({ state: "degraded", epoch: 3, reason: "opaque" }, {
      state: "loginPending", epoch: 3, auth: "signedOut", loginMethods: ["browser", "device", "apiKey"],
    });
    expect(state?.reason).toBeUndefined();
    expect(state?.auth).toBe("signedOut");
    expect(runtimeStatusPresentation(state, true)?.canRetry).toBe(true);
    expect(runtimeResponseDeadlinePending(state)).toBe(false);
  });
  it("leaves initial loading to the startup surface while preserving actionable failures", () => {
    expect(runtimeStatusPresentation({ state: "loading", epoch: 1 }, true)).toBeNull();
    expect(runtimeStatusPresentation({ state: "degraded", epoch: 1 }, true)?.canRetry).toBe(true);
  });
  it.each(states.flatMap((from) => states.map((to) => [from, to] as const)))
    ("accepts core transition %s → %s", (from, to) => {
      const result = runtimeStateFromEvent({ state: from, epoch: 2, reason: "old" }, authority, {
        tab_id: "owned", session_binding_nonce: "binding", state: { state: to, epoch: 3, reason: "fault" },
      });
      expect(result).toEqual(to === "loaded" ? { state: "loaded", epoch: 3 } : { state: to, epoch: 3, reason: "fault" });
    });

  it.each(["loading", "unloaded", "degraded"] as const)("presents runtime status for %s", (state) => {
    expect(runtimeStatusPresentation({ state, epoch: 3, reason: "opaque-fault" })).toMatchObject({
      severity: state === "loading" ? "transient" : "action_needed", details: "opaque-fault", canRetry: state !== "loading",
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
    expect(runtimeStatusPresentation(loading)).toMatchObject({ severity: "transient", details: "pipe closed", canRetry: false });
    expect(beginRuntimeRetry(loading)).toBe(loading);
    const failed = runtimeRetryFailed(loading, "resume refused");
    expect(failed).toEqual({ state: "degraded", epoch: 1, reason: "resume refused" });
    expect(runtimeStatusPresentation(failed)?.canRetry).toBe(true);
  });

  it("only authoritative loaded state clears the failure", () => {
    const state = runtimeStateFromSnapshot({ state: "loading", epoch: 2, reason: "fault" }, { state: "loaded", epoch: 2 });
    expect(state).toEqual({ state: "loaded", epoch: 2 });
    expect(runtimeStatusPresentation(state)).toBeNull();
    expect(beginRuntimeRetry(state)).toBe(state);
    expect(runtimeRetryFailed(state, "late rejection")).toBe(state);
  });

  it("keeps a response deadline pending and lets a later success clear the slow state", () => {
    const deadline = { state: "degraded" as const, epoch: 2, reason: "Deadline: opaque" };
    expect(runtimeResponseDeadlinePending(deadline)).toBe(true);
    expect(runtimeStatusPresentation(deadline)).toMatchObject({ severity: "transient", canRetry: false });
    expect(runtimeStatusPresentation(deadline, true)).toBeNull();

    const recovered = runtimeStateFromSnapshot(deadline, { state: "loaded", epoch: 2 });
    expect(runtimeResponseDeadlinePending(recovered)).toBe(false);
    expect(runtimeStatusPresentation(recovered)).toBeNull();
  });

  it("keeps terminal runtime failures actionable", () => {
    const failed = { state: "degraded" as const, epoch: 2, reason: "ProcessExited: runtime exited" };
    expect(runtimeResponseDeadlinePending(failed)).toBe(false);
    expect(runtimeStatusPresentation(failed)).toMatchObject({ severity: "action_needed", canRetry: true });
  });

  it("keeps non-runtime tabs silent and reports missing fault reasons honestly", () => {
    expect(runtimeStateFromSnapshot(null, null)).toBeNull();
    expect(runtimeStatusPresentation(null)).toBeNull();
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


describe("runtime status presentation", () => {
  it("has no badge for absent or healthy state, including recovery clearing", () => {
    expect(runtimeStatusPresentation(null)).toBeNull();
    const failed: AgentRuntimeState = { state: "degraded", epoch: 1, reason: "opaque-fault" };
    expect(runtimeStatusPresentation(failed)).toMatchObject({ severity: "action_needed", canRetry: true });
    const retrying = beginRuntimeRetry(failed);
    expect(runtimeStatusPresentation(retrying)).toMatchObject({ severity: "transient", canRetry: false });
    const recovered = runtimeStateFromSnapshot(retrying, { state: "loaded", epoch: 2 });
    expect(runtimeStatusPresentation(recovered)).toBeNull();
  });

  it("keeps a prior fault diagnostic from giving a transient state failure severity", () => {
    expect(runtimeStatusPresentation({ state: "loading", epoch: 2, reason: "PipeFailure: opaque-detail" }))
      .toMatchObject({ severity: "transient", canRetry: false, details: "PipeFailure: opaque-detail" });
  });

  it.each([null, undefined, "", "   "])("omits empty diagnostics: %s", reason => {
    expect(runtimeStatusPresentation({ state: "degraded", epoch: 1, reason })?.details).toBeNull();
  });

  it.each([
    ["SizeLimit", "Runtime hook state size limit"],
    ["SpawnFailed", "Runtime could not start:"],
    ["ProcessExited", "Runtime exited (code 1):"],
    ["PipeEof", "Runtime connection closed"],
    ["PipeFailure", "Runtime stdout connection failed:"],
    ["InvalidFrame", "Runtime sent an invalid response:"],
    ["Deadline", "Runtime did not respond in time"],
    ["HookFailed", "Provider operation failed:"],
    ["Refused", "Provider refused:"],
    ["PrewriteRefused", "Input was not sent:"],
    ["Superseded", "Runtime session changed; this operation was cancelled"],
  ])("classifies equivalent %s faults and retains their opaque diagnostics", (code, prefix) => {
    const state = { state: "degraded" as const, epoch: 1 };
    const coded = runtimeStatusPresentation({ ...state, reason: `${code}: opaque-detail` })!;
    const described = runtimeStatusPresentation({ ...state, reason: `${prefix} opaque-detail` })!;
    expect(coded.reason).toBe(described.reason);
    expect(coded.details).toBe(`${code}: opaque-detail`);
    expect(described.details).toBe(`${prefix} opaque-detail`);
    expect(coded.reason).not.toBe(coded.details);
  });

  it("settles history only when a loading replay completes or fails", () => {
    expect(runtimeHistorySettled("loading", "complete")).toBe(true);
    expect(runtimeHistorySettled("loading", "failed")).toBe(true);
    expect(runtimeHistorySettled("loading", "loading")).toBe(false);
    expect(runtimeHistorySettled(undefined, "complete")).toBe(false);
    expect(runtimeHistorySettled("complete", "complete")).toBe(false);
    expect(runtimeHistorySettled("failed", "loading")).toBe(false);
  });

  it("carries history progress through runtime state events", () => {
    const loading = runtimeStateFromEvent(null, authority, {
      tab_id: "owned", session_binding_nonce: "binding",
      state: { state: "loaded", epoch: 2, history: "loading", historyPages: 1 },
    });
    expect(loading?.history).toBe("loading");
    expect(loading?.historyPages).toBe(1);
    const done = runtimeStateFromEvent(loading, authority, {
      tab_id: "owned", session_binding_nonce: "binding",
      state: { state: "loaded", epoch: 2, history: "complete", historyPages: 4 },
    });
    expect(runtimeHistorySettled(loading?.history, done?.history)).toBe(true);
  });
});


it("retries a failed history page while the conversation remains loaded", () => {
  const state:AgentRuntimeState={state:"loaded",epoch:3,history:"failed",historyPages:8,historyError:"fixture-page-nine"};
  expect(runtimeStateFromSnapshot(null,state)).toMatchObject(state);
  expect(runtimeStatusPresentation(state)).toMatchObject({details:state.historyError,canRetry:true});
  const loading=beginRuntimeRetry(state);
  expect(loading).toMatchObject({state:"loaded",history:"loading",epoch:3,historyPages:8});
  expect(beginRuntimeRetry(loading)).toBe(loading);
  expect(runtimeRetryFailed(loading,"fixture-retry-error")).toMatchObject({state:"loaded",history:"failed",historyError:"fixture-retry-error"});
  expect(runtimeStateFromEvent(loading,authority,{tab_id:"owned",session_binding_nonce:"binding",state:{state:"loaded",epoch:3,history:"complete",historyPages:12}})).toMatchObject({state:"loaded",history:"complete",historyPages:12});
});
