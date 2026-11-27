/** Pure frontend projection model for cloud machine storage usage, retention tiers,
 * idle hot cost, cold archival inclusion, owner-confirmed volume upgrades, and local disk-full alerts.
 * Keeps usage views and the cloud meter in strict agreement with `CloudUsageReply.storage`. */

export type CloudStorageTier = "hot" | "evicting" | "cold" | "recoverable_deleted" | "purged";

export interface ProviderSnapshotResidualView {
  provider: string;
  snapshots_remaining: number;
  earliest_expiry_at?: number | null;
  latest_expiry_at?: number | null;
  auto_backup_enabled: boolean;
  individual_delete_supported: boolean;
  erasure_state: "none" | "bounded_provider_expiry";
  checked_at: number;
}

export interface CloudMachineStorageUsageWire {
  machine_id: string;
  create_request_id?: string | null;
  name?: string | null;
  created_at_ms?: number | null;
  tier: CloudStorageTier;
  allocated_gb: number;
  used_bytes?: number | null;
  used_gb?: number | null;
  hot_monthly_cost_cents: number;
  hot_accrued_cents: number;
  cold_included: boolean;
  stopped_at?: number | null;
  hot_until_at?: number | null;
  cold_since_at?: number | null;
  purge_at?: number | null;
  archive_id?: string | null;
  archive_bytes?: number | null;
  archive_generation: number;
  disk_full: boolean;
  volume_upgrade_max_gb: number;
  snapshot_residual?: ProviderSnapshotResidualView | null;
}

export interface CloudStorageUsageSummaryWire {
  hot_gb_month_rate_cents: number;
  hot_days: number;
  purge_days: number;
  purge_warning_days: [number, number];
  cold_archive_included: boolean;
  total_allocated_gb: number;
  total_used_bytes: number;
  total_hot_monthly_cost_cents: number;
  machines: CloudMachineStorageUsageWire[];
}

export interface CloudMachineStorageRow {
  machineId: string;
  machineName: string | null;
  tier: CloudStorageTier;
  tierLabel: string;
  allocatedGb: number;
  usedLabel: string;
  utilizationPct: number | null;
  hotMonthlyCostLabel: string;
  coldIncludedNote: string;
  retentionDeadlineAt: number | null;
  retentionDeadlineLabel: string | null;
  canUpgradeVolume: boolean;
  upgradeMaxGb: number;
  diskFullLocal: boolean;
  residualSnapshotNotice: string | null;
}

export interface CloudStorageMeterSummary {
  totalAllocatedGb: number;
  totalUsedGb: number;
  totalHotMonthlyCostLabel: string;
  hotRateLabel: string;
  coldPolicyLabel: string;
  autoArchiveLabel: string;
  anyMachineDiskFull: boolean;
  rows: CloudMachineStorageRow[];
}

export function formatMoneyCents(cents: number, currency = "USD"): string {
  const safe = Number.isFinite(cents) && cents >= 0 ? cents : 0;
  const code = currency.trim().toUpperCase() || "USD";
  const prefix = code === "USD" ? "$" : `${code} `;
  return `${prefix}${(safe / 100).toFixed(2)}`;
}

export function formatStorageBytes(bytes: number | null | undefined): string {
  if (typeof bytes !== "number" || !Number.isFinite(bytes) || bytes < 0) return "—";
  const gib = bytes / (1024 * 1024 * 1024);
  if (gib >= 1) return `${gib.toFixed(2)} GB`;
  const mib = bytes / (1024 * 1024);
  if (mib >= 1) return `${mib.toFixed(1)} MB`;
  const kib = bytes / 1024;
  return `${kib.toFixed(0)} KB`;
}

export function storageTierLabel(tier: CloudStorageTier): string {
  switch (tier) {
    case "hot": return "Hot";
    case "evicting": return "Archiving";
    case "cold": return "Archived - wakes in ~2 min";
    case "recoverable_deleted": return "Archived - wakes in ~2 min";
    case "purged": return "Purged";
  }
}

export function formatAutoArchiveLabel(hotDays: number | null | undefined): string {
  const safe = typeof hotDays === "number" && Number.isFinite(hotDays) && hotDays > 0 ? hotDays : 1;
  return safe === 1
    ? "auto-archives after 1 day asleep"
    : `auto-archives after ${safe} days asleep`;
}

export function validateOwnerVolumeUpgrade(
  currentAllocatedGb: number,
  requestedGb: number,
  maxGb: number,
  ownerConfirmed: boolean,
): { ok: true; newSizeGb: number } | { ok: false; reason: string } {
  if (!ownerConfirmed) {
    return { ok: false, reason: "Owner confirmation is required before increasing volume capacity." };
  }
  if (!Number.isSafeInteger(requestedGb) || requestedGb <= currentAllocatedGb) {
    return { ok: false, reason: `New volume size must be a whole GB strictly greater than ${currentAllocatedGb} GB.` };
  }
  if (requestedGb > maxGb) {
    return { ok: false, reason: `New volume size cannot exceed ${maxGb} GB.` };
  }
  return { ok: true, newSizeGb: requestedGb };
}

export function projectCloudStorageMeter(
  summary: CloudStorageUsageSummaryWire,
  currency = "USD",
  machineNamesById?: ReadonlyMap<string, string>,
): CloudStorageMeterSummary {
  const rows: CloudMachineStorageRow[] = summary.machines.map(m => {
    const rawName = m.name?.trim() || null;
    const resolvedName =
      rawName && rawName !== m.machine_id
        ? rawName
        : (machineNamesById?.get(m.machine_id)?.trim() || rawName);
    const capacityBytes = m.allocated_gb * 1024 * 1024 * 1024;
    const utilizationPct = m.tier === "hot" && capacityBytes > 0 && typeof m.used_bytes === "number"
      ? Math.min(100, Math.round((m.used_bytes / capacityBytes) * 100))
      : null;
    const retentionDeadlineAt = m.tier === "hot" ? (m.hot_until_at ?? null) : (m.purge_at ?? null);
    const retentionDeadlineLabel = m.tier === "hot" && m.hot_until_at
      ? "Moves to cold archive"
      : (m.tier === "cold" || m.tier === "recoverable_deleted") && m.purge_at
        ? "Timed purge"
        : null;
    const residualSnapshotNotice = m.snapshot_residual?.erasure_state === "bounded_provider_expiry"
      ? `${m.snapshot_residual.snapshots_remaining} provider snapshot(s) expire automatically by provider retention window`
      : null;

    return {
      machineId: m.machine_id,
      machineName: resolvedName,
      tier: m.tier,
      tierLabel: storageTierLabel(m.tier),
      allocatedGb: m.allocated_gb,
      usedLabel: m.tier === "hot" || m.tier === "evicting"
        ? `${formatStorageBytes(m.used_bytes)} / ${m.allocated_gb} GB`
        : `${formatStorageBytes(m.archive_bytes ?? m.used_bytes)} archived`,
      utilizationPct,
      hotMonthlyCostLabel: m.tier === "hot" || m.tier === "evicting"
        ? `${formatMoneyCents(m.hot_monthly_cost_cents, currency)}/mo`
        : "Included ($0.00)",
      coldIncludedNote: "Cold archival included",
      retentionDeadlineAt,
      retentionDeadlineLabel,
      canUpgradeVolume: m.tier === "hot" && m.allocated_gb < m.volume_upgrade_max_gb,
      upgradeMaxGb: m.volume_upgrade_max_gb,
      diskFullLocal: m.disk_full,
      residualSnapshotNotice,
    };
  });

  return {
    totalAllocatedGb: summary.total_allocated_gb,
    totalUsedGb: Math.round((summary.total_used_bytes / (1024 * 1024 * 1024)) * 100) / 100,
    totalHotMonthlyCostLabel: `${formatMoneyCents(summary.total_hot_monthly_cost_cents, currency)}/mo`,
    hotRateLabel: `${formatMoneyCents(summary.hot_gb_month_rate_cents, currency)}/GB-month`,
    coldPolicyLabel: `${summary.hot_days}d hot -> ${summary.purge_days}d cold (included)`,
    autoArchiveLabel: formatAutoArchiveLabel(summary.hot_days),
    anyMachineDiskFull: rows.some(r => r.diskFullLocal),
    rows,
  };
}
