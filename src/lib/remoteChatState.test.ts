import { describe, expect, finish, it } from "../test/standaloneTest";
import { isMeshDisabledError, PeerTabRelayError, remoteChatFailure, remoteChatState, remoteMachineName } from "./remoteChatState";
import { MuxAdmissionError, OutcomeUnknown, RetryableMuxError } from "./streamMux";
import { TransportError } from "./transportClient";

const relay = (status: number, code?: string, bytesWritten?: boolean) =>
  remoteChatFailure(new PeerTabRelayError(status, "/api/mesh/relay-to", "opaque", code, bytesWritten), 1);

describe("remote chat state", () => {
  for (const [label, failure, state] of [
    ["removed", remoteChatFailure(new Error("/api/mesh/relay-to → HTTP 404: peer_not_paired: opaque")), "not-paired"],
    ["refused", relay(403), "refused"],
    ["old HTTP", relay(426), "too-old"],
    ["old protocol", relay(400, "version_unsupported"), "too-old"],
    ["protocol mismatch", relay(426, "peer_relay_protocol_mismatch"), "too-old"],
    ["signed out", relay(401), "signed-out"],
    ["network", remoteChatFailure(new TypeError("Failed to fetch")), "unreachable"],
    ["refused connection", remoteChatFailure(new Error("connection refused")), "unreachable"],
    ["timeout", remoteChatFailure(new Error("request timed out")), "unreachable"],
    ["relay 5xx", relay(503), "unreachable"],
    ["retryable mux", remoteChatFailure(new RetryableMuxError("/api/mesh/relay-to", "Mux connection lost")), "unreachable"],
    ["mux admission", remoteChatFailure(new MuxAdmissionError("/api/mesh/relay-to")), "unreachable"],
    ["outcome unknown", remoteChatFailure(new OutcomeUnknown("POST", "/api/mesh/relay-to")), "unreachable"],
    ["admission unknown", remoteChatFailure(new Error("admission_unknown: send request is still unconfirmed")), "unreachable"],
    ["mesh disabled", remoteChatFailure(new TransportError(503, "/api/mesh/relay-to", "Mesh networking is disabled")), "unreachable"],
    ["missing tab", relay(404), "generic"],
    ["local auth", remoteChatFailure(new Error("/api/mesh/relay-to → HTTP 401: opaque")), "generic"],
    ["unknown", relay(400), "generic"],
  ] as const) {
    it(label, () => expect(remoteChatState("online", failure)).toBe(state));
  }
  it("uses known presence before any request", () => {
    expect(remoteChatState("offline", relay(503))).toBe("offline");
    expect(remoteChatState("not-paired")).toBe("not-paired");
    expect(remoteChatState("signed-out")).toBe("signed-out");
    expect(remoteChatState("online")).toBe(null);
    expect(remoteChatState()).toBe(null);
  });
  it("only trusts evidence that no write happened", () => {
    expect(relay(504, "mesh_transport_submit_unconfirmed").neverWritten).toBe(false);
    expect(relay(403, "peer_relay_not_authorized", true).neverWritten).toBe(false);
    expect(relay(403, "peer_relay_not_authorized", false).neverWritten).toBe(true);
    expect(remoteChatFailure(new Error("/api/mesh/relay-to → HTTP 404: peer_not_paired: opaque")).neverWritten).toBe(true);
    expect(remoteChatFailure(new Error("/api/mesh/relay-to → HTTP 502: opaque")).neverWritten).toBe(false);
    expect(remoteChatFailure(new MuxAdmissionError("/api/mesh/relay-to")).neverWritten).toBe(true);
    expect(remoteChatFailure(new OutcomeUnknown("POST", "/api/mesh/relay-to")).neverWritten).toBe(false);
    expect(remoteChatFailure(new TransportError(503, "/api/mesh/relay-to", "Mesh networking is disabled")).neverWritten).toBe(true);
    expect(isMeshDisabledError(new TransportError(503, "/api/mesh/peers", "Mesh networking is disabled"))).toBe(true);
    expect(isMeshDisabledError(new TransportError(503, "/api/mesh/peers", "peer_unroutable"))).toBe(false);
  });
  it("names the machine from the peer, tab or fallback", () => {
    const id = "cea6e137-dead-dead-dead-aabbccddeeff";
    expect(remoteMachineName(id, "ROG", "Old label")).toBe("ROG");
    expect(remoteMachineName(id, id, "Old label")).toBe("Old label");
    expect(remoteMachineName(id, id, " ")).toBe("the other computer");
  });
});

finish();
