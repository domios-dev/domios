import { describe, expect, it } from "vitest";
import { createMuxReadCapacity, EMPTY_MUX_POSTS, initialMuxRequest, MUX_READ_CAPACITY, muxPostEnqueue, muxPostRemove, muxPostStart, muxPostTraffic, muxRequestAdmission, muxRequestPolicy, readWhenMuxCapacityAvailable, transitionMuxRequest } from "./streamMuxRequestModel";

it("capacity waiters handle a release before registration, cancellation and origin closure", async () => {
  const capacity = createMuxReadCapacity();
  const late = capacity.capture();
  capacity.released();
  await late();
  const controller = new AbortController();
  const cancelled = capacity.capture()(controller.signal);
  controller.abort();
  await expect(cancelled).rejects.toMatchObject({name:"AbortError"});
  const closed = capacity.capture();
  capacity.close(new Error("origin ended"));
  await expect(closed()).rejects.toThrow("origin ended");
});

it("opted-in reads wait for each capacity release and stop on dispatched failures", async () => {
  const capacity = createMuxReadCapacity();
  let attempts = 0;
  let refused!: () => void;
  const firstRefusal = new Promise<void>(resolve => { refused = resolve; });
  const read = async () => {
    attempts++;
    if (attempts === 1) { refused(); throw {[MUX_READ_CAPACITY]:capacity.capture()}; }
    throw new Error("dispatched failure");
  };
  const result = readWhenMuxCapacityAvailable(read,new AbortController().signal).catch(error=>error);
  await firstRefusal;
  expect(attempts).toBe(1);
  capacity.released();
  expect(await result).toMatchObject({message:"dispatched failure"});
  capacity.released();
  expect(attempts).toBe(2);
});

describe("mux request outcomes", () => {
  it("permits one repost only after a proven stale-session refusal", () => {
    let state = initialMuxRequest(false);
    state = transitionMuxRequest(state, { type: "posted" }).state;
    let step = transitionMuxRequest(state, { type: "refused", status: 410 });
    expect(step.action).toBe("reopen");
    expect(step.state.phase).toBe("registered");
    step = transitionMuxRequest(transitionMuxRequest(step.state, { type: "posted" }).state, { type: "refused", status: 410 });
    expect(step.action).toBe("not-dispatched");
    expect(step.state.phase).toBe("lost");
  });

  it("never reposts an uncertain mutation, including failure before its ack", () => {
    for (const acked of [false, true]) {
      let state = transitionMuxRequest(initialMuxRequest(false), { type: "posted" }).state;
      if (acked) state = transitionMuxRequest(state, { type: "acked" }).state;
      const step = transitionMuxRequest(state, { type: "lost" });
      expect(step.action).toBe("outcome-unknown");
      expect(step.state.phase).toBe("lost");
    }
    const read = transitionMuxRequest(initialMuxRequest(true), { type: "posted" }).state;
    expect(transitionMuxRequest(read, { type: "lost" }).action).toBe("retryable");
    const mutation = transitionMuxRequest(initialMuxRequest(false), { type: "posted" }).state;
    expect(transitionMuxRequest(mutation, { type: "refused", status: 500 }).action).toBe("outcome-unknown");
  });

  it("holds early response frames until the ack, including an entire early response", () => {
    let state = transitionMuxRequest(initialMuxRequest(true), { type: "posted" }).state;
    for (const stage of ["head", "body", "done"] as const) {
      const step = transitionMuxRequest(state, { type: "frame", stage });
      expect(step.action).toBe("buffer");
      expect(step.state.phase).toBe("posted");
      state = step.state;
    }
    const ack = transitionMuxRequest(state, { type: "acked" });
    expect(ack.state.phase).toBe("done");
    expect(ack.action).toBe("deliver");
  });

  it("cancels a read and detaches a posted mutation; terminal events are inert", () => {
    for (const [read, action] of [[true, "cancel-read"], [false, "detach-mutation"]] as const) {
      const posted = transitionMuxRequest(initialMuxRequest(read), { type: "posted" }).state;
      const cancelled = transitionMuxRequest(posted, { type: "cancel" });
      expect(cancelled.action).toBe(action);
      expect(transitionMuxRequest(cancelled.state, { type: "lost" }).action).toBe("none");
    }
    expect(transitionMuxRequest(initialMuxRequest(false), { type: "cancel" }).action).toBe("none");
  });
});

describe("mux POST admission", () => {
  it("keeps interactive POST slots available while uploads wait for their ack", () => {
    let state = EMPTY_MUX_POSTS;
    for (const id of ["a", "b", "c", "d"]) state = muxPostEnqueue(state, id, muxPostTraffic("interactive", true));
    let step = muxPostStart(state); state = step.state;
    expect(step.ids).toEqual(["a", "b"]);
    state = muxPostEnqueue(state, "send", "interactive");
    step = muxPostStart(state); state = step.state;
    expect(step.ids).toEqual(["send"]);
    state = muxPostRemove(state, "a");
    expect(muxPostStart(state).ids).toEqual(["c"]);
  });

  it("bounds the interactive burst so bulk makes progress", () => {
    let state = EMPTY_MUX_POSTS;
    for (let i = 0; i < 16; i++) state = muxPostEnqueue(state, String(i), "interactive");
    state = muxPostEnqueue(state, "bulk", "bulk");
    const order: string[] = [];
    for (let i = 0; i < 5; i++) {
      const step = muxPostStart(state); state = step.state; order.push(...step.ids);
      for (const id of step.ids) state = muxPostRemove(state, id);
    }
    expect(order.indexOf("bulk")).toBeLessThanOrEqual(8);
  });

  it("reserves in-flight capacity for interactive responses independently of streams", () => {
    expect(muxRequestAdmission(false, "bulk", 256, 48, 48)).toBe(false);
    expect(muxRequestAdmission(false, "interactive", 256, 48, 48)).toBe(false);
    expect(muxRequestAdmission(false, "interactive", 256, 64, 48, true)).toBe(true);
    expect(muxRequestAdmission(false, "interactive", 0, 64, 48)).toBe(false);
    expect(muxRequestAdmission(true, "bulk", 256, 0, 0)).toBe(false);
  });
});

describe("mux request authority and traffic class", () => {
  it("declares only specific read POSTs cancellable and bulk", () => {
    for (const path of ["/api/tabs/t/preview/read", "/api/tabs/t/preview/check", "/api/tabs/t/chat-raw/start", "/api/tabs/t/chat-raw/history", "/api/tabs/t/chat-raw/backfill", "/api/essence-maps"]) {
      expect(muxRequestPolicy("POST", path)).toMatchObject({ traffic: "bulk", cancellable: true });
    }
    for (const path of ["/api/tabs/t/chat-send", "/api/tabs/t/send", "/api/tabs/t/chat-raw/stop", "/api/workspaces/w/focus", "/api/secrets/backend"]) {
      expect(muxRequestPolicy("POST", path)).toMatchObject({ traffic: "interactive", cancellable: false });
    }
    expect(muxRequestPolicy("DELETE", "/api/logs")).toMatchObject({ traffic: "interactive", cancellable: false });
    expect(muxRequestPolicy("GET", "/api/logs?limit=10")).toMatchObject({ traffic: "bulk", cancellable: true });
  });

  it("uses the original relay operation without granting arbitrary POST cancellation", () => {
    expect(muxRequestPolicy("POST", "/api/mesh/relay-to", JSON.stringify({ method: "GET", path: "/api/sessions/transcript" })))
      .toMatchObject({ traffic: "bulk", cancellable: true });
    expect(muxRequestPolicy("POST", "/api/mesh/relay-to", JSON.stringify({ method: "POST", path: "/api/tabs/t/chat-send" })))
      .toMatchObject({ traffic: "interactive", cancellable: false });
    expect(muxRequestPolicy("POST", "/api/mesh/relay-to", "bad")).toMatchObject({ cancellable: false });
  });
});
