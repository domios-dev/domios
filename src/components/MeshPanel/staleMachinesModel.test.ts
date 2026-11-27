import { describe, expect, it } from "vitest";
import sharedFixtureJson from "../../../tests/fixtures/mesh-stale-cleanup.json";
import type { CloudReceipt } from "../../lib/cloudMachines/types";
import type { MachineRowView, MachineStatus } from "./accountMachines";
import {
  CLOUD_EXCLUSION_NOTE,
  DEFAULT_STALE_UNREACHABLE_DAYS,
  SECONDS_PER_DAY,
  formatStaleMachineNames,
  isLiveManagedCloudMachine,
  selectStaleMachinePlan,
  selectStaleMachineRows,
  staleCleanupActionLabel,
  staleCleanupConfirmationView,
  type CloudMachineLink,
} from "./staleMachinesModel";

const NOW = 1_800_000_000;

interface SharedStaleFixture {
  now: number;
  local_installation_id: string;
  cloud_machines: Array<{
    machine_id: string;
    name: string;
    provider: string;
    size: string;
    state: CloudReceipt["state"];
    sleep_mode: CloudReceipt["sleep_mode"];
    installation_id: string | null;
    peer_instance_id: string | null;
    storage_tier: CloudReceipt["storage_tier"];
  }>;
  pair_bindings: Array<[string, string]>;
  registry_rows: Array<{
    installation_id: string;
    display_name: string | null;
    active: boolean;
    presence: "present" | "signed_out";
    authenticated_at: number;
    membership_expires_at: number | null;
    endpoint_expires_at: number | null;
    online: boolean;
  }>;
  expected_selected: Array<{
    installation_id: string;
    display_name: string;
    reason: "signed_out" | "membership_expired" | "unreachable";
  }>;
  expected_excluded_cloud_ids: string[];
}

const SHARED_FIXTURE = sharedFixtureJson as SharedStaleFixture;

function makeRow(
  key: string,
  name: string,
  status: MachineStatus,
  overrides: Partial<MachineRowView> = {},
): MachineRowView {
  return {
    key,
    name,
    status,
    detail: status,
    online: status === "linked" ? true : false,
    target: { installation_id: key, key_id: "a".repeat(64), key_generation: 1, slot_revision: 1 },
    controls: {
      revoke: status === "revoked"
        ? null
        : { installation_id: key, key_id: "a".repeat(64), key_generation: 1, slot_revision: 1 },
    },
    pairInstanceId: null,
    lastSeenAt: NOW - 3600,
    ...overrides,
  };
}

function fixtureRowToView(
  entry: SharedStaleFixture["registry_rows"][number],
  now: number,
  pairByInstallation: ReadonlyMap<string, string>,
): MachineRowView {
  let status: MachineStatus = "unreachable";
  if (!entry.active) {
    status = "revoked";
  } else if (entry.presence === "signed_out") {
    status = "signed_out";
  } else if (entry.membership_expires_at !== null && entry.membership_expires_at <= now) {
    status = "expired";
  } else if (entry.online) {
    status = "linked";
  } else if (entry.endpoint_expires_at === null) {
    status = "endpoint_missing";
  } else if (entry.endpoint_expires_at <= now) {
    status = "endpoint_stale";
  }
  return makeRow(entry.installation_id, entry.display_name ?? entry.installation_id, status, {
    online: entry.online,
    lastSeenAt: entry.authenticated_at,
    pairInstanceId: pairByInstallation.get(entry.installation_id) ?? null,
  });
}

describe("staleMachinesModel", () => {
  it("selects expired, signed-out, and long-unreachable machines while sparing online, recent offline, and revoked", () => {
    const rows: MachineRowView[] = [
      makeRow("online-1", "Main Desk", "linked", { online: true, lastSeenAt: NOW - 60 }),
      makeRow("connecting-1", "Booting", "linking", { online: false, lastSeenAt: NOW - 60 }),
      makeRow("revoked-1", "Already Gone", "revoked", { lastSeenAt: NOW - 30 * SECONDS_PER_DAY }),
      makeRow("over-limit-1", "Locked", "over_limit", { lastSeenAt: NOW - 30 * SECONDS_PER_DAY }),
      makeRow("signed-out-1", "Old Mac", "signed_out", { lastSeenAt: NOW - 3600 }),
      makeRow("expired-1", "Expired Box", "expired", { lastSeenAt: NOW - 7200 }),
      makeRow("member-exp-1", "Lease Gone", "membership_expired", { lastSeenAt: NOW - 10_800 }),
      makeRow("unreach-recent", "Brief Offline", "unreachable", { lastSeenAt: NOW - 2 * SECONDS_PER_DAY }),
      makeRow("unreach-old", "Dead Server", "endpoint_stale", {
        lastSeenAt: NOW - (DEFAULT_STALE_UNREACHABLE_DAYS + 1) * SECONDS_PER_DAY,
      }),
    ];

    const selected = selectStaleMachineRows(rows, { now: NOW });
    expect(selected.map((c) => [c.installationId, c.reason])).toEqual([
      ["unreach-old", "unreachable"],
      ["member-exp-1", "membership_expired"],
      ["expired-1", "membership_expired"],
      ["signed-out-1", "signed_out"],
    ]);
  });

  it("applies olderThanDays across stale categories without ever selecting online machines", () => {
    const rows: MachineRowView[] = [
      makeRow("online-old", "Online", "linked", { online: true, lastSeenAt: NOW - 60 * SECONDS_PER_DAY }),
      makeRow("signed-out-new", "Recent Out", "signed_out", { lastSeenAt: NOW - 2 * SECONDS_PER_DAY }),
      makeRow("expired-old", "Old Expired", "expired", { lastSeenAt: NOW - 20 * SECONDS_PER_DAY }),
      makeRow("offline-2d", "Offline 2d", "endpoint_missing", { lastSeenAt: NOW - 2 * SECONDS_PER_DAY }),
    ];

    expect(
      selectStaleMachineRows(rows, { now: NOW, olderThanDays: 14 }).map((c) => c.installationId),
    ).toEqual(["expired-old"]);
    expect(
      selectStaleMachineRows(rows, { now: NOW, olderThanDays: 0 }).map((c) => c.installationId),
    ).toEqual(["expired-old", "offline-2d", "signed-out-new"]);
  });

  it("excludes awake, asleep, cold/archived, draining, and waking cloud machines by linkage while including ordinary stale and deleted/purged machines (shared fixture)", () => {
    const pairByInstallation = new Map(SHARED_FIXTURE.pair_bindings);
    const rows = SHARED_FIXTURE.registry_rows
      .filter((entry) => entry.installation_id !== SHARED_FIXTURE.local_installation_id)
      .map((entry) => fixtureRowToView(entry, SHARED_FIXTURE.now, pairByInstallation));
    const cloudMachines: CloudMachineLink[] = SHARED_FIXTURE.cloud_machines;

    const plan = selectStaleMachinePlan(rows, {
      now: SHARED_FIXTURE.now,
      cloudMachines,
    });

    expect(plan.candidates.map((c) => ({
      installation_id: c.installationId,
      display_name: c.name,
      reason: c.reason,
    }))).toEqual(SHARED_FIXTURE.expected_selected);

    expect(plan.excludedCloud.map((c) => c.installationId)).toEqual(
      SHARED_FIXTURE.expected_excluded_cloud_ids,
    );

    // Direct helper agrees with the plan candidates and never branches on names.
    expect(
      selectStaleMachineRows(rows, { now: SHARED_FIXTURE.now, cloudMachines }).map(
        (c) => c.installationId,
      ),
    ).toEqual(SHARED_FIXTURE.expected_selected.map((c) => c.installation_id));
  });

  it("treats every non-deleted, non-purged cloud machine state as live and excludes it even with --older-than 0", () => {
    const liveStates: Array<Pick<CloudReceipt, "state" | "storage_tier">> = [
      { state: "creating", storage_tier: "hot" },
      { state: "enrolling", storage_tier: "hot" },
      { state: "awake", storage_tier: "hot" },
      { state: "draining", storage_tier: "hot" },
      { state: "asleep", storage_tier: "hot" },
      { state: "asleep", storage_tier: "evicting" },
      { state: "asleep", storage_tier: "cold" },
      { state: "waking", storage_tier: "hot" },
      { state: "out_of_credits", storage_tier: "cold" },
      { state: "failed", storage_tier: "hot" },
    ];
    for (const entry of liveStates) {
      expect(isLiveManagedCloudMachine(entry)).toBe(true);
    }
    expect(isLiveManagedCloudMachine({ state: "deleted", storage_tier: "recoverable_deleted" })).toBe(false);
    expect(isLiveManagedCloudMachine({ state: "deleted", storage_tier: "purged" })).toBe(false);
    expect(isLiveManagedCloudMachine({ state: "asleep", storage_tier: "purged" })).toBe(false);

    const rows = liveStates.map((_, idx) =>
      makeRow(`inst-live-${idx}`, `ordinary-name-${idx}`, "expired", {
        lastSeenAt: NOW - 30 * SECONDS_PER_DAY,
      }),
    );
    const cloudMachines: CloudMachineLink[] = liveStates.map((entry, idx) => ({
      machine_id: `cm-live-${idx}`,
      state: entry.state,
      installation_id: `inst-live-${idx}`,
      peer_instance_id: null,
      storage_tier: entry.storage_tier,
    }));

    const plan = selectStaleMachinePlan(rows, {
      now: NOW,
      olderThanDays: 0,
      cloudMachines,
    });
    expect(plan.candidates).toEqual([]);
    expect(plan.excludedCloud).toHaveLength(liveStates.length);
  });

  it("formats cleanup button labels, bounded confirmation machine lists, and cloud exclusion copy", () => {
    expect(staleCleanupActionLabel(1)).toBe("Clean up 1 stale machine");
    expect(staleCleanupActionLabel(12)).toBe("Clean up 12 stale machines");
    expect(formatStaleMachineNames([{ name: "A" }, { name: "B" }, { name: "C" }], 2)).toEqual({
      visible: ["A", "B"],
      overflow: 1,
    });

    const withoutCloud = staleCleanupConfirmationView([{ name: "Old Mac" }], 0);
    expect(withoutCloud.removalNames).toEqual(["Old Mac"]);
    expect(withoutCloud.cloudExclusionNote).toBeNull();

    const withCloud = staleCleanupConfirmationView(
      [{ name: "fly-ordinary-laptop" }, { name: "Old Mac" }],
      3,
    );
    expect(withCloud.removalNames).toEqual(["fly-ordinary-laptop", "Old Mac"]);
    expect(withCloud.cloudExclusionNote).toBe(CLOUD_EXCLUSION_NOTE);
    expect(withCloud.cloudExclusionNote).toContain(
      "Cloud machines are never removed here; delete them from the cloud drawer",
    );
  });
});
