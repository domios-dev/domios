import type { CloudReceipt } from "../../lib/cloudMachines/types";
import type { MachineRowView, MachineStatus } from "./accountMachines";

export const SECONDS_PER_DAY = 86_400;
export const DEFAULT_STALE_UNREACHABLE_DAYS = 7;
export const CLOUD_EXCLUSION_NOTE =
  "Cloud machines are never removed here; delete them from the cloud drawer.";

export type StaleCleanupReason = "signed_out" | "membership_expired" | "unreachable";

export interface StaleMachineCandidate {
  installationId: string;
  name: string;
  status: MachineStatus;
  reason: StaleCleanupReason;
  lastSeenAt: number | null;
}

export interface ExcludedCloudMachine {
  installationId: string;
  name: string;
  status: MachineStatus;
  wouldBeReason: StaleCleanupReason;
}

export interface StaleMachineSelection {
  candidates: StaleMachineCandidate[];
  excludedCloud: ExcludedCloudMachine[];
}

export type CloudMachineLink = Pick<
  CloudReceipt,
  "machine_id" | "state" | "installation_id" | "peer_instance_id" | "storage_tier"
>;

export interface SelectStaleMachineOptions {
  now?: number;
  olderThanDays?: number;
  cloudMachines?: readonly CloudMachineLink[];
}

export type RemoveStaleRowStatus =
  | "would_remove"
  | "removed"
  | "already_removed"
  | "refused_self"
  | "refused_online"
  | "refused_cloud"
  | "skipped"
  | "not_found";

export interface RemoveStaleRowResult {
  installation_id: string;
  display_name: string | null;
  reason: StaleCleanupReason | null;
  status: RemoveStaleRowStatus;
  slot_revision: number | null;
}

export interface RemoveStaleResult {
  dry_run: boolean;
  matched: number;
  removed: number;
  already_removed: number;
  skipped: number;
  excluded_cloud_count?: number;
  rows: RemoveStaleRowResult[];
}

export function isLiveManagedCloudMachine(
  machine: Pick<CloudReceipt, "state" | "storage_tier">,
): boolean {
  return machine.state !== "deleted" && machine.storage_tier !== "purged";
}

export function collectProtectedCloudIds(
  cloudMachines?: readonly CloudMachineLink[],
): ReadonlySet<string> {
  const ids = new Set<string>();
  for (const machine of cloudMachines ?? []) {
    if (!isLiveManagedCloudMachine(machine)) continue;
    const installationId = machine.installation_id?.trim();
    if (installationId) ids.add(installationId);
    const peerInstanceId = machine.peer_instance_id?.trim();
    if (peerInstanceId) ids.add(peerInstanceId);
    const machineId = machine.machine_id.trim();
    if (machineId) ids.add(machineId);
  }
  return ids;
}

export function isRowBackedByCloudMachine(
  row: Pick<MachineRowView, "key" | "target" | "pairInstanceId">,
  protectedCloudIds: ReadonlySet<string>,
): boolean {
  if (protectedCloudIds.size === 0) return false;
  if (protectedCloudIds.has(row.key)) return true;
  if (row.target?.installation_id && protectedCloudIds.has(row.target.installation_id)) {
    return true;
  }
  if (row.pairInstanceId && protectedCloudIds.has(row.pairInstanceId)) {
    return true;
  }
  return false;
}

export function classifyStaleRow(
  row: MachineRowView,
  now: number,
  olderThanDays?: number,
  protectedCloudIds?: ReadonlySet<string>,
): StaleCleanupReason | null {
  if (protectedCloudIds && isRowBackedByCloudMachine(row, protectedCloudIds)) {
    return null;
  }
  if (row.status === "revoked" || row.online === true || row.status === "linked" || row.status === "linking") {
    return null;
  }
  if (!row.controls.revoke) return null;

  let reason: StaleCleanupReason | null = null;
  if (row.status === "signed_out") {
    reason = "signed_out";
  } else if (row.status === "expired" || row.status === "membership_expired") {
    reason = "membership_expired";
  } else if (
    row.status === "unreachable"
    || row.status === "endpoint_stale"
    || row.status === "endpoint_missing"
  ) {
    reason = "unreachable";
  }
  if (!reason) return null;

  const age = row.lastSeenAt != null ? Math.max(0, now - row.lastSeenAt) : Number.POSITIVE_INFINITY;
  if (olderThanDays !== undefined) {
    return age >= olderThanDays * SECONDS_PER_DAY ? reason : null;
  }
  if (reason === "unreachable") {
    return age >= DEFAULT_STALE_UNREACHABLE_DAYS * SECONDS_PER_DAY ? reason : null;
  }
  return reason;
}

/** Pure selection of stale machines and excluded live managed cloud machines for List view cleanup. */
export function selectStaleMachinePlan(
  rows: readonly MachineRowView[],
  options: SelectStaleMachineOptions = {},
): StaleMachineSelection {
  const now = options.now ?? Math.floor(Date.now() / 1000);
  const protectedCloudIds = collectProtectedCloudIds(options.cloudMachines);
  const candidates: StaleMachineCandidate[] = [];
  const excludedCloud: ExcludedCloudMachine[] = [];
  for (const row of rows) {
    const baseReason = classifyStaleRow(row, now, options.olderThanDays);
    if (!baseReason) continue;
    if (isRowBackedByCloudMachine(row, protectedCloudIds)) {
      excludedCloud.push({
        installationId: row.key,
        name: row.name,
        status: row.status,
        wouldBeReason: baseReason,
      });
      continue;
    }
    candidates.push({
      installationId: row.key,
      name: row.name,
      status: row.status,
      reason: baseReason,
      lastSeenAt: row.lastSeenAt,
    });
  }
  candidates.sort((a, b) => {
    const aSeen = a.lastSeenAt ?? 0;
    const bSeen = b.lastSeenAt ?? 0;
    return aSeen - bSeen || a.installationId.localeCompare(b.installationId);
  });
  excludedCloud.sort((a, b) => a.installationId.localeCompare(b.installationId));
  return { candidates, excludedCloud };
}

/** Pure selection of stale machines eligible for batch cleanup from the Mesh List view. */
export function selectStaleMachineRows(
  rows: readonly MachineRowView[],
  options: SelectStaleMachineOptions = {},
): StaleMachineCandidate[] {
  return selectStaleMachinePlan(rows, options).candidates;
}

export function staleCleanupActionLabel(count: number): string {
  return `Clean up ${count} stale machine${count === 1 ? "" : "s"}`;
}

export function formatStaleMachineNames(
  machines: readonly Pick<StaleMachineCandidate, "name">[],
  maxVisible = 8,
): { visible: string[]; overflow: number } {
  const limit = Math.max(1, maxVisible);
  const visible = machines.slice(0, limit).map((machine) => machine.name);
  return { visible, overflow: Math.max(0, machines.length - visible.length) };
}

export interface StaleCleanupConfirmationView {
  summary: string;
  removalNames: string[];
  overflow: number;
  cloudExclusionNote: string | null;
}

export function staleCleanupConfirmationView(
  machines: readonly Pick<StaleMachineCandidate, "name">[],
  excludedCloudCount = 0,
  maxVisible = 8,
): StaleCleanupConfirmationView {
  const count = machines.length;
  const noun = count === 1 ? "machine" : "machines";
  const { visible, overflow } = formatStaleMachineNames(machines, maxVisible);
  return {
    summary: `Remove ${count} stale ${noun} from your mesh? This machine and online machines are never affected.`,
    removalNames: visible,
    overflow,
    cloudExclusionNote: excludedCloudCount > 0 ? CLOUD_EXCLUSION_NOTE : null,
  };
}
