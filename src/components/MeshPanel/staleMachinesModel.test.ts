import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { MachineRowView, MachineStatus } from "./accountMachines";
import {
  DEFAULT_STALE_UNREACHABLE_DAYS,
  SECONDS_PER_DAY,
  formatStaleMachineNames,
  selectStaleMachineRows,
  staleCleanupActionLabel,
} from "./staleMachinesModel";

const NOW = 1_800_000_000;

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
    assert.deepEqual(
      selected.map((c) => [c.installationId, c.reason]),
      [
        ["unreach-old", "unreachable"],
        ["member-exp-1", "membership_expired"],
        ["expired-1", "membership_expired"],
        ["signed-out-1", "signed_out"],
      ],
    );
  });

  it("applies olderThanDays across stale categories without ever selecting online machines", () => {
    const rows: MachineRowView[] = [
      makeRow("online-old", "Online", "linked", { online: true, lastSeenAt: NOW - 60 * SECONDS_PER_DAY }),
      makeRow("signed-out-new", "Recent Out", "signed_out", { lastSeenAt: NOW - 2 * SECONDS_PER_DAY }),
      makeRow("expired-old", "Old Expired", "expired", { lastSeenAt: NOW - 20 * SECONDS_PER_DAY }),
      makeRow("offline-2d", "Offline 2d", "endpoint_missing", { lastSeenAt: NOW - 2 * SECONDS_PER_DAY }),
    ];

    assert.deepEqual(
      selectStaleMachineRows(rows, { now: NOW, olderThanDays: 14 }).map((c) => c.installationId),
      ["expired-old"],
    );
    assert.deepEqual(
      selectStaleMachineRows(rows, { now: NOW, olderThanDays: 0 }).map((c) => c.installationId),
      ["expired-old", "offline-2d", "signed-out-new"],
    );
  });

  it("formats cleanup button labels and bounded confirmation machine name lists", () => {
    assert.equal(staleCleanupActionLabel(1), "Clean up 1 stale machine");
    assert.equal(staleCleanupActionLabel(12), "Clean up 12 stale machines");
    assert.deepEqual(
      formatStaleMachineNames([{ name: "A" }, { name: "B" }, { name: "C" }], 2),
      { visible: ["A", "B"], overflow: 1 },
    );
  });
});
