// Run: npx tsx src/components/Surface/surfaceHttpProtocol.test.ts

import assert from "node:assert/strict";
import {
  callReply,
  disconnectedNotice,
  INITIAL_RECONNECT,
  MAX_IMMEDIATE_RECONNECTS,
  nextReconnect,
  RECONNECTED_NOTICE,
  reconnectOnSignal,
  refusedNotice,
  streamOpened,
  streamOutcomeForStatus,
  surfaceCallUrl,
  surfaceStreamEvent,
  surfaceStreamUrl,
  type ReconnectState,
  type StreamOutcome,
} from "./surfaceHttpProtocol";

assert.equal(
  surfaceStreamUrl("http://127.0.0.1:7686/", "tab 1", { w: 640, h: 480, dpr: 2 }),
  "http://127.0.0.1:7686/api/surfaces/tab%201/stream?w=640&h=480&dpr=2",
  "the stream URL carries the size and an encoded tab id",
);
assert.equal(surfaceCallUrl("https://host", "t1"), "https://host/api/surfaces/t1/call");

const state = { targetId: "target-1", owner: "Jev", title: "Login", url: "https://a.test/", activity: "typing", needsHuman: "none", profile: "clean", profiles: ["Work", 3] };
assert.deepEqual(surfaceStreamEvent({ event: "state", data: JSON.stringify(state) }), {
  type: "state",
  state: { ...state, profiles: ["Work"] },
}, "state keeps known fields and drops non-string profiles");
assert.deepEqual(surfaceStreamEvent({ event: "state", data: "{}" }), { type: "state", state: { owner: "" } }, "a state without owner still parses");

const metadata = { deviceWidth: 800, deviceHeight: 600, offsetTop: 0, pageScaleFactor: 1, timestamp: 1 };
assert.deepEqual(surfaceStreamEvent({ event: "frame", data: JSON.stringify({ data: "QUJD", metadata }) }), { type: "frame", frame: { data: "QUJD", metadata } });
assert.equal(surfaceStreamEvent({ event: "frame", data: JSON.stringify({ data: "QUJD" }) }), null, "a frame without metadata is dropped");
assert.equal(surfaceStreamEvent({ event: "frame", data: JSON.stringify({ data: 1, metadata }) }), null, "a frame without base64 data is dropped");

assert.deepEqual(surfaceStreamEvent({ event: "notice", data: "{\"text\":\"hi\",\"tone\":\"error\"}" }), { type: "notice", notice: { text: "hi", tone: "error" } });
assert.deepEqual(surfaceStreamEvent({ event: "notice", data: "{\"text\":\"hi\",\"tone\":\"loud\"}" }), { type: "notice", notice: { text: "hi" } }, "an unknown tone is dropped");
assert.deepEqual(
  surfaceStreamEvent({ event: "notice", data: JSON.stringify({ text: "Preview paused while the window is hidden", kind: "preview_paused", detail: "no frame: x" }) }),
  { type: "notice", notice: { text: "Preview paused while the window is hidden", kind: "preview_paused", detail: "no frame: x" } },
  "a typed notice keeps its kind and technical detail",
);
assert.deepEqual(
  surfaceStreamEvent({ event: "notice", data: JSON.stringify({ text: "hi", kind: "mystery", detail: 3 }) }),
  { type: "notice", notice: { text: "hi" } },
  "an unknown kind and a non-string detail are dropped",
);
assert.equal(surfaceStreamEvent({ event: "notice", data: "not json" }), null, "malformed JSON is ignored");
assert.equal(surfaceStreamEvent({ event: "message", data: "{}" }), null, "unknown events are ignored");

assert.deepEqual(callReply(200, "{\"result\":{\"current\":\"Work\"}}"), { ok: true, result: { current: "Work" } });
assert.deepEqual(callReply(200, "{\"result\":null}"), { ok: true, result: null });
assert.deepEqual(callReply(200, "{}"), { ok: true, result: null }, "a missing result is null");
assert.deepEqual(callReply(200, "{\"error\":\"no page\"}"), { ok: false, message: "no page" });
assert.deepEqual(callReply(200, "{\"error\":{\"code\":\"x\",\"message\":\"bad args\"}}"), { ok: false, message: "bad args" });
assert.deepEqual(callReply(409, "{\"ok\":false,\"error\":{\"code\":\"not_a_stream_surface\",\"message\":\"Tab t1 is not a stream surface\"}}"), { ok: false, message: "Tab t1 is not a stream surface" });
assert.deepEqual(callReply(500, "boom"), { ok: false, message: "HTTP 500: boom" });
assert.deepEqual(callReply(502, ""), { ok: false, message: "HTTP 502" });
assert.deepEqual(callReply(200, "<html>"), { ok: false, message: "Malformed surface reply" });

assert.deepEqual(streamOutcomeForStatus(409, "{\"error\":\"not a stream surface\"}"), { kind: "refused", message: "not a stream surface" });
assert.equal(streamOutcomeForStatus(401, "").kind, "refused");
assert.equal(streamOutcomeForStatus(404, "").kind, "refused");
assert.equal(streamOutcomeForStatus(429, "").kind, "dropped", "rate limiting heals");
assert.equal(streamOutcomeForStatus(503, "").kind, "dropped");

const failed: StreamOutcome = { kind: "dropped", delivered: false, message: "connection refused" };
let reconnect: ReconnectState = INITIAL_RECONNECT;
for (let attempt = 1; attempt <= MAX_IMMEDIATE_RECONNECTS; attempt += 1) {
  const step = nextReconnect(reconnect, failed);
  assert.equal(step.action, "retry", `failure ${attempt} retries at once`);
  assert.equal(step.notice, undefined, "a retry within the cap is silent");
  reconnect = step.state;
}
const parked = nextReconnect(reconnect, failed);
assert.equal(parked.action, "park", "the cap parks the stream");
assert.deepEqual(parked.notice, disconnectedNotice("connection refused"), "parking shows a visible notice");
assert.equal(nextReconnect(parked.state, failed).notice, undefined, "a parked stream does not repeat the notice");

assert.deepEqual(reconnectOnSignal(INITIAL_RECONNECT), { retry: false, state: INITIAL_RECONNECT }, "a live stream ignores reachability signals");
const resumed = reconnectOnSignal(parked.state);
assert.equal(resumed.retry, true, "a parked stream retries on a reachability signal");
assert.equal(resumed.state.failures, 0, "the retry budget refills");
const reopened = streamOpened(resumed.state);
assert.deepEqual(reopened.notice, RECONNECTED_NOTICE, "recovery after a notice is announced");
assert.equal(streamOpened(reopened.state).notice, undefined, "a plain open is silent");

const afterDelivery = nextReconnect({ ...INITIAL_RECONNECT, failures: MAX_IMMEDIATE_RECONNECTS }, { kind: "dropped", delivered: true, message: "ended" });
assert.equal(afterDelivery.action, "retry", "a stream that delivered events refills the budget before retrying");
assert.equal(afterDelivery.state.failures, 1);

const refused = nextReconnect(INITIAL_RECONNECT, { kind: "refused", message: "not a stream surface" });
assert.equal(refused.action, "stop");
assert.deepEqual(refused.notice, refusedNotice("not a stream surface"));
assert.equal(reconnectOnSignal(refused.state).retry, false, "a refused stream never retries");

console.log("✓ surfaceHttpProtocol tests green");
