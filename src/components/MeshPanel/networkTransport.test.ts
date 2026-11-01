import { describe, expect, it } from "vitest";
import { differentNetwork, transportState, type NetworkTransportStatus } from "./networkTransport";

const status = (state: NetworkTransportStatus["state"]): NetworkTransportStatus => ({
  display_name: "Private transport", state, identity: null, network_name: null, user: null,
  addresses: [], peer_listener: true, encrypted: true, setup_actions: [],
});

describe("network transport projection", () => {
  it("keeps actionable states distinct", () => {
    for (const state of ["not_installed", "not_running", "permission_needed", "signed_out", "unknown"] as const) {
      expect(transportState(status(state))).toEqual({ kind: state });
    }
    expect(transportState(status("starting"))).toEqual({ kind: "checking" });
    expect(transportState(status("connected"))).toEqual({ kind: "connected", tailnet: null });
  });
  it("compares only known identities of the same transport", () => {
    const a = { kind: "second", network_id: "a" };
    expect(differentNetwork(a, { ...a, network_id: "b" })).toBe(true);
    expect(differentNetwork(a, a)).toBe(false);
    expect(differentNetwork(a, undefined)).toBe(false);
    expect(differentNetwork(null, a)).toBe(false);
    expect(differentNetwork(a, { kind: "other", network_id: "b" })).toBe(false);
  });
});
