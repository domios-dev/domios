import { describe, expect, it } from "vitest";
import type { CloudReceipt } from "../../lib/cloudMachines/types";
import { accountCanvas } from "./accountCanvas";
import { myMachinesView } from "./accountMachines";
import { lenovoRow, macRow, NOW, pair, selfRow, snapshot } from "./accountMachines.testdata";
import {
  classifyAccountStatus,
  collapsedGroupSummary,
  countProjectedMeshMachines,
  filterMeshMachineRows,
  groupMeshMachineRows,
  isGroupCollapsed,
  MESH_MACHINE_GROUP_ORDER,
  projectMeshMachineRows,
  toggleCollapsedGroup,
} from "./meshMachineListModel";
import type { PeerStatus } from "./useMeshData";

function makePeer(instanceId: string, name: string, patch: Partial<PeerStatus> = {}): PeerStatus {
  return {
    instance_id: instanceId,
    name,
    ip: "100.64.0.10",
    api_port: 7685,
    online: true,
    access_level: "full",
    last_seen: null,
    relay_protocol_version: 2,
    domios_version: "1.24.0",
    ...patch,
  };
}

function makeCloud(id: string, name: string, state: CloudReceipt["state"], patch: Partial<CloudReceipt> = {}): CloudReceipt {
  return {
    kind: "cloud",
    machine_id: id,
    name,
    provider: "fly_machines",
    size: "small",
    state,
    sleep_mode: "stop_billed",
    uptime_secs: state === "awake" ? 600 : 0,
    billing_notice: "",
    ...patch,
  };
}

describe("meshMachineListModel", () => {
  it("classifies every account status according to the spec", () => {
    expect(classifyAccountStatus("linked", true)).toBe("online");
    expect(classifyAccountStatus("linking", false)).toBe("online");
    expect(classifyAccountStatus("linked", true, true)).toBe("offline");
    expect(classifyAccountStatus("unreachable", false)).toBe("offline");
    expect(classifyAccountStatus("waiting", false)).toBe("offline");
    expect(classifyAccountStatus("endpoint_missing", false)).toBe("offline");
    expect(classifyAccountStatus("endpoint_stale", false)).toBe("offline");
    expect(classifyAccountStatus("revoked", false)).toBe("offline");

    for (const status of [
      "auth_refused",
      "membership_expired",
      "peer_needs_update",
      "expired",
      "signed_out",
      "over_limit",
      "unverified",
      "blocked",
      "other_account",
    ] as const) {
      expect(classifyAccountStatus(status, true)).toBe("needs_attention");
      expect(classifyAccountStatus(status, false)).toBe("needs_attention");
    }
  });

  it("groups machines in spec order and applies per-group sorting rules", () => {
    const expiredRecent = { ...lenovoRow, installation_id: "inst-exp-1", display_name: "Old ThinkPad", membership_expires_at: NOW - 10, authenticated_at: NOW - 120 };
    const expiredOlder = { ...lenovoRow, installation_id: "inst-exp-2", display_name: "Archive Box", membership_expires_at: NOW - 10, authenticated_at: NOW - 7200 };
    const signedOutMid = { ...lenovoRow, installation_id: "inst-so-1", display_name: "Mini", presence: "signed_out" as const, authenticated_at: NOW - 600 };
    const offlineNewer = { ...lenovoRow, installation_id: "inst-off-1", display_name: "Work laptop", authenticated_at: NOW - 300, endpoint: null };
    const offlineOlder = { ...lenovoRow, installation_id: "inst-off-2", display_name: "Garage Pi", authenticated_at: NOW - 3600, endpoint: null };
    const onlineMac = { ...macRow, installation_id: "inst-on-1", display_name: "MacBook Air", authenticated_at: NOW - 50 };
    const onlineNuc = { ...macRow, installation_id: "inst-on-2", display_name: "Kitchen NUC", authenticated_at: NOW - 10 };
    const signedOutHeartbeatRow = { ...macRow, installation_id: "inst-so-2", display_name: "SignedOut OnlineHeartbeat", presence: "signed_out" as const, authenticated_at: NOW - 20 };

    const state = snapshot({
      registry: {
        observed_at: NOW,
        rows: [
          selfRow,
          onlineMac,
          onlineNuc,
          signedOutHeartbeatRow,
          offlineOlder,
          offlineNewer,
          expiredOlder,
          signedOutMid,
          expiredRecent,
        ],
      },
      pairs: [
        pair("peer-mac", "MacBook Air", { installation_id: onlineMac.installation_id, online: true }),
        pair("peer-nuc", "Kitchen NUC", { installation_id: onlineNuc.installation_id, online: true }),
        pair("peer-so", "SignedOut OnlineHeartbeat", { installation_id: signedOutHeartbeatRow.installation_id, online: true }),
      ],
    });

    const peers: PeerStatus[] = [
      makePeer("peer-mac", "MacBook Air", { online: true }),
      makePeer("peer-nuc", "Kitchen NUC", { online: true }),
      makePeer("peer-so", "SignedOut OnlineHeartbeat", { online: true, last_seen: new Date((NOW - 20) * 1000).toISOString() }),
      makePeer("peer-cloud-awake", "fly-builder", { online: true }),
    ];

    const cloudMachines: CloudReceipt[] = [
      makeCloud("cm-awake", "fly-builder", "awake", { peer_instance_id: "peer-cloud-awake" }),
      makeCloud("cm-asleep", "fly-scratch", "asleep"),
      makeCloud("cm-waking", "fly-qa", "waking"),
      makeCloud("cm-ooc", "fly-render", "out_of_credits"),
      makeCloud("cm-deleted", "fly-deleted", "deleted"),
    ];

    const canvas = accountCanvas({
      view: myMachinesView({ account: "signed_in", snapshot: state, now: NOW }),
      peers,
      loadFailed: false,
      listFailed: false,
      failures: new Map(),
    });

    const rows = projectMeshMachineRows({
      thisMachine: { name: "Studio", ip: "192.168.1.10" },
      peers,
      account: {
        selfName: "Studio",
        canvas,
        actions: { onPlans: () => {}, onDismiss: () => {} },
      },
      cloudMachines,
      localVersion: "1.24.0",
    });

    expect(countProjectedMeshMachines(peers, canvas, cloudMachines)).toBe(rows.length);
    const groups = groupMeshMachineRows(rows);
    expect(groups.map((g) => g.id)).toEqual(MESH_MACHINE_GROUP_ORDER);

    expect(groups[0].rows.map((r) => r.name)).toEqual(["Studio"]);
    expect(groups[1].rows.map((r) => r.name)).toEqual(["Kitchen NUC", "MacBook Air"]);
    expect(groups[2].rows.map((r) => r.name)).toEqual(["fly-builder", "fly-qa", "fly-render", "fly-scratch"]);
    expect(groups[2].rows.find((r) => r.name === "fly-render")?.tone).toBe("warning");
    expect(groups[3].rows.map((r) => r.name)).toEqual([
      "Old ThinkPad",
      "Archive Box",
      "SignedOut OnlineHeartbeat",
      "Mini",
    ]);
    expect(groups[4].rows.map((r) => r.name)).toEqual(["Work laptop", "Garage Pi"]);

    const signedOutRow = groups[3].rows.find((r) => r.name === "SignedOut OnlineHeartbeat")!;
    expect(signedOutRow.canOpen).toBe(false);
    expect(rows.some((r) => r.name === "fly-deleted")).toBe(false);
  });

  it("auto-collapses offline only above 3 rows unless overridden, summarizes collapsed rows, and filters by name/status/version", () => {
    expect(isGroupCollapsed("offline", 3, {})).toBe(false);
    expect(isGroupCollapsed("offline", 4, {})).toBe(true);
    expect(isGroupCollapsed("offline", 4, { offline: false })).toBe(false);
    expect(isGroupCollapsed("online", 10, {})).toBe(false);

    const expandedOffline = toggleCollapsedGroup({}, "offline", 4);
    expect(expandedOffline).toEqual({ offline: false });
    expect(toggleCollapsedGroup(expandedOffline, "offline", 4)).toEqual({ offline: true });

    const rows = projectMeshMachineRows({
      thisMachine: { name: "Studio", ip: "127.0.0.1" },
      peers: [
        makePeer("p1", "Work laptop", { online: false, domios_version: "1.24.0" }),
        makePeer("p2", "Garage Pi", { online: false, domios_version: "1.23.3" }),
        makePeer("p3", "Vacation iMac", { online: false, domios_version: "1.22.1" }),
        makePeer("p4", "dev-vm-old", { online: false, domios_version: "1.20.4" }),
      ],
      localVersion: "1.24.0",
    });
    const offlineRows = groupMeshMachineRows(rows).find((g) => g.id === "offline")!.rows;
    expect(collapsedGroupSummary(offlineRows, 2)).toBe("dev-vm-old, Garage Pi and 2 more");
    expect(collapsedGroupSummary(offlineRows, 4)).toBe("dev-vm-old, Garage Pi, Vacation iMac, Work laptop");

    expect(filterMeshMachineRows(rows, "1.22.1").map((r) => r.name)).toEqual(["Vacation iMac"]);
    expect(filterMeshMachineRows(rows, "garage").map((r) => r.name)).toEqual(["Garage Pi"]);
  });

  it("uses the cloud machine's own timestamps rather than the owner's older account auth_time", () => {
    const sevenDaysAgoSec = NOW - 7 * 24 * 3600;
    const sixHoursAgoSec = NOW - 6 * 3600;
    const sixHoursAgoMs = sixHoursAgoSec * 1000;
    const cloudAccountRow = {
      ...lenovoRow,
      installation_id: "inst-cloud-fresh",
      display_name: "fly-fresh",
      authenticated_at: sevenDaysAgoSec,
      admitted_at: sixHoursAgoSec,
    };
    const state = snapshot({
      registry: { observed_at: NOW, rows: [selfRow, cloudAccountRow] },
      pairs: [pair("peer-cloud-fresh", "fly-fresh", { installation_id: "inst-cloud-fresh", online: false })],
    });
    const peers: PeerStatus[] = [
      makePeer("peer-cloud-fresh", "fly-fresh", { online: false, last_seen: null }),
    ];
    const cloudMachines: CloudReceipt[] = [
      makeCloud("cm-fresh", "fly-fresh", "asleep", {
        installation_id: "inst-cloud-fresh",
        peer_instance_id: "peer-cloud-fresh",
        created_at_ms: sixHoursAgoMs,
        last_seen_at_ms: sixHoursAgoMs,
      }),
    ];
    const canvas = accountCanvas({
      view: myMachinesView({ account: "signed_in", snapshot: state, now: NOW }),
      peers,
      loadFailed: false,
      listFailed: false,
      failures: new Map(),
    });
    const rows = projectMeshMachineRows({
      thisMachine: { name: "Studio", ip: "127.0.0.1" },
      peers,
      account: {
        selfName: "Studio",
        canvas,
        actions: { onPlans: () => {}, onDismiss: () => {} },
      },
      cloudMachines,
      localVersion: "1.24.0",
    });

    const cloudRow = rows.find((r) => r.id === "cloud:cm-fresh");
    expect(cloudRow).toBeDefined();
    expect(cloudRow?.lastSeenMs).toBe(sixHoursAgoMs);
    expect(cloudRow?.lastSeenMs).not.toBe(sevenDaysAgoSec * 1000);
  });

  it("matches storage timestamps by machine_id and does not swap when two cloud machines share a name", () => {
    const olderStoppedSec = NOW - 5 * 3600;
    const newerStoppedSec = NOW - 1 * 3600;
    const rows = projectMeshMachineRows({
      thisMachine: { name: "Studio", ip: "127.0.0.1" },
      peers: [],
      cloudMachines: [
        makeCloud("cm-dup-a", "fly-dup", "asleep"),
        makeCloud("cm-dup-b", "fly-dup", "asleep"),
        makeCloud("cm-dup-unmatched", "fly-dup", "asleep"),
      ],
      cloudUsage: {
        used: 1,
        allowance: 10,
        purchased: 0,
        remaining: 9,
        cap: 10,
        burn_per_size: { small: 1 },
        storage: {
          hot_gb_month_rate_cents: 30,
          hot_days: 1,
          purge_days: 90,
          purge_warning_days: [7, 1],
          cold_archive_included: true,
          total_allocated_gb: 20,
          total_used_bytes: 0,
          total_hot_monthly_cost_cents: 600,
          machines: [
            {
              machine_id: "cm-dup-a",
              name: "fly-dup",
              tier: "hot",
              allocated_gb: 10,
              hot_monthly_cost_cents: 300,
              hot_accrued_cents: 0,
              cold_included: true,
              stopped_at: olderStoppedSec,
              archive_generation: 1,
              disk_full: false,
              volume_upgrade_max_gb: 500,
            },
            {
              machine_id: "cm-dup-b",
              name: "fly-dup",
              tier: "hot",
              allocated_gb: 10,
              hot_monthly_cost_cents: 300,
              hot_accrued_cents: 0,
              cold_included: true,
              stopped_at: newerStoppedSec,
              archive_generation: 1,
              disk_full: false,
              volume_upgrade_max_gb: 500,
            },
          ],
        },
      },
      localVersion: "1.24.0",
    });

    expect(rows.find((r) => r.id === "cloud:cm-dup-a")?.lastSeenMs).toBe(olderStoppedSec * 1000);
    expect(rows.find((r) => r.id === "cloud:cm-dup-b")?.lastSeenMs).toBe(newerStoppedSec * 1000);
    expect(rows.find((r) => r.id === "cloud:cm-dup-unmatched")?.lastSeenMs).toBeNull();
  });
});
