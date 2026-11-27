import { describe, expect, it } from "vitest";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import type { CloudReceipt, EnrichedPairedPeer } from "../../lib/cloudMachines/types";
import { peerStatus, resolvedPeerName, unpairedCloudMachines } from "./peerPickerModel";

const PEER_ID = "3efa6b47-cbad-4243-b381-86e57930319a";
const INST_ID = "e45c9e4ff42143f0bee45f1f75b11a9c";

function makeCloudPeer(overrides: Partial<EnrichedPairedPeer> = {}): EnrichedPairedPeer {
  return {
    instance_id: PEER_ID,
    instance_name: "quill-fly-124-go",
    addresses: [],
    api_port: 0,
    public_key: "pk-cloud",
    paired_at: "2026-01-01T00:00:00Z",
    last_seen: null,
    online: true,
    is_primary: false,
    access_level: "full",
    web_ui: null,
    last_active_ms: null,
    relay_protocol_version: RELAY_PROTOCOL_VERSION,
    kind: "cloud",
    cloud: {
      kind: "cloud",
      machine_id: "cloud-DPGKwwYH",
      name: "quill-fly-124-go",
      provider: "fly_machines",
      size: "small",
      state: "awake",
      sleep_mode: "stop",
      uptime_secs: 60,
      billing_notice: "",
    },
    ...overrides,
  };
}

function makeCloudReceipt(overrides: Partial<CloudReceipt> = {}): CloudReceipt {
  return {
    kind: "cloud",
    machine_id: "cloud-DPGKwwYH",
    name: "quill-fly-124-go",
    provider: "fly_machines",
    size: "small",
    state: "awake",
    sleep_mode: "stop",
    uptime_secs: 60,
    installation_id: INST_ID,
    peer_instance_id: PEER_ID,
    billing_notice: "",
    ...overrides,
  };
}

describe("peerPickerModel", () => {
  it("deduplicates an enrolled cloud machine by peer_instance_id, cloud.machine_id, or installations binding", () => {
    const peer = makeCloudPeer();
    expect(unpairedCloudMachines([peer], [makeCloudReceipt()])).toEqual([]);

    // Even if cloud_machines receipt lacks peer_instance_id, cloud.machine_id or installations prevents duplicates.
    expect(
      unpairedCloudMachines([peer], [makeCloudReceipt({ peer_instance_id: null })]),
    ).toEqual([]);
    expect(
      unpairedCloudMachines(
        [makeCloudPeer({ kind: undefined, cloud: undefined })],
        [makeCloudReceipt({ peer_instance_id: null })],
        [{
          installation_id: INST_ID,
          instance_id: PEER_ID,
          name: "",
          active: true,
          signed_out: false,
          online: true,
          membership_expires_at: null,
          endpoint_expires_at: null,
        }],
      ),
    ).toEqual([]);

    // Truly unpaired sleeping cloud machine still appears once.
    const sleeping = makeCloudReceipt({
      machine_id: "cloud-asleep",
      name: "sleeping-box",
      installation_id: "inst-asleep",
      peer_instance_id: "peer-asleep",
      state: "asleep",
    });
    expect(unpairedCloudMachines([peer], [makeCloudReceipt(), sleeping])).toEqual([sleeping]);
  });

  it("prefers cloud.name over raw UUID instance_name or fallback Machine", () => {
    const peer = makeCloudPeer({ instance_name: PEER_ID });
    expect(resolvedPeerName(peer, undefined)).toBe("quill-fly-124-go");
    expect(resolvedPeerName(peer, "fly-generic-host")).toBe("quill-fly-124-go");
  });

  it("reflects online when attached and degrades honestly when detached or reconnecting", () => {
    const attached = makeCloudPeer({ online: true });
    expect(peerStatus(attached, "online")).toEqual({ text: "online", icon: "dot", online: true, cloud: true });
    expect(peerStatus(attached, "reconnecting")).toEqual({ text: "reconnecting", icon: "dots", online: false, cloud: true });
    expect(peerStatus(attached, "unreachable")).toEqual({ text: "unreachable", icon: "dot", online: false, cloud: true });

    const detached = makeCloudPeer({ online: false });
    expect(peerStatus(detached, "offline")).toEqual({ text: "offline", icon: "dot", online: false, cloud: true });
    expect(peerStatus(detached, undefined)).toEqual({ text: "offline", icon: "dot", online: false, cloud: true });
  });
});
