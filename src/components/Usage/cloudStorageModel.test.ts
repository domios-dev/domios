import { describe, expect, it } from "vitest";
import {
  projectCloudStorageMeter,
  validateOwnerVolumeUpgrade,
  type CloudStorageUsageSummaryWire,
} from "./cloudStorageModel";

describe("cloudStorageModel", () => {
  const sampleSummary: CloudStorageUsageSummaryWire = {
    hot_gb_month_rate_cents: 15,
    hot_days: 14,
    purge_days: 90,
    purge_warning_days: [7, 1],
    cold_archive_included: true,
    total_allocated_gb: 10,
    total_used_bytes: Math.round(9.9 * 1024 ** 3),
    total_hot_monthly_cost_cents: 150,
    machines: [
      {
        machine_id: "cm_hot_1",
        tier: "hot",
        allocated_gb: 10,
        used_bytes: Math.round(9.9 * 1024 ** 3),
        used_gb: 9.9,
        hot_monthly_cost_cents: 150,
        hot_accrued_cents: 70,
        cold_included: true,
        stopped_at: 1_795_700_000,
        hot_until_at: 1_795_700_000 + 14 * 86_400,
        cold_since_at: null,
        purge_at: null,
        archive_id: null,
        archive_bytes: null,
        archive_generation: 1,
        disk_full: true,
        volume_upgrade_max_gb: 500,
        snapshot_residual: null,
      },
      {
        machine_id: "cm_cold_2",
        tier: "cold",
        allocated_gb: 0,
        used_bytes: 12 * 1024 * 1024,
        used_gb: 0.011,
        hot_monthly_cost_cents: 0,
        hot_accrued_cents: 0,
        cold_included: true,
        stopped_at: null,
        hot_until_at: null,
        cold_since_at: 1_795_700_000,
        purge_at: 1_795_700_000 + 90 * 86_400,
        archive_id: "arc_cold_2",
        archive_bytes: 12 * 1024 * 1024,
        archive_generation: 2,
        disk_full: false,
        volume_upgrade_max_gb: 500,
        snapshot_residual: {
          provider: "fly_machines",
          snapshots_remaining: 2,
          earliest_expiry_at: 1_795_900_000,
          latest_expiry_at: 1_796_100_000,
          auto_backup_enabled: true,
          individual_delete_supported: false,
          erasure_state: "bounded_provider_expiry",
          checked_at: 1_795_700_000,
        },
      },
    ],
  };

  it("projects per-machine disk usage, retention tier/deadlines, hot cost, cold inclusion, and local disk-full", () => {
    const view = projectCloudStorageMeter(sampleSummary, "USD");
    expect(view.totalAllocatedGb).toBe(10);
    expect(view.totalHotMonthlyCostLabel).toBe("$1.50/mo");
    expect(view.hotRateLabel).toBe("$0.15/GB-month");
    expect(view.coldPolicyLabel).toBe("14d hot -> 90d cold (included)");
    expect(view.anyMachineDiskFull).toBe(true);

    const hotRow = view.rows.find(r => r.machineId === "cm_hot_1")!;
    expect(hotRow.tierLabel).toBe("Hot volume");
    expect(hotRow.usedLabel).toBe("9.90 GB / 10 GB");
    expect(hotRow.utilizationPct).toBe(99);
    expect(hotRow.hotMonthlyCostLabel).toBe("$1.50/mo");
    expect(hotRow.diskFullLocal).toBe(true);
    expect(hotRow.canUpgradeVolume).toBe(true);
    expect(hotRow.retentionDeadlineLabel).toBe("Moves to cold archive");

    const coldRow = view.rows.find(r => r.machineId === "cm_cold_2")!;
    expect(coldRow.tierLabel).toBe("Cold archive");
    expect(coldRow.hotMonthlyCostLabel).toBe("Included ($0.00)");
    expect(coldRow.diskFullLocal).toBe(false);
    expect(coldRow.canUpgradeVolume).toBe(false);
    expect(coldRow.retentionDeadlineLabel).toBe("Timed purge");
    expect(coldRow.residualSnapshotNotice).toContain("2 provider snapshot(s)");
  });

  it("requires explicit owner confirmation and grow-only bounds for volume upgrades", () => {
    expect(validateOwnerVolumeUpgrade(10, 25, 500, false).ok).toBe(false);
    expect(validateOwnerVolumeUpgrade(10, 10, 500, true).ok).toBe(false);
    expect(validateOwnerVolumeUpgrade(10, 501, 500, true).ok).toBe(false);
    expect(validateOwnerVolumeUpgrade(10, 25, 500, true)).toEqual({ ok: true, newSizeGb: 25 });
  });
});
