import type { MachineRowView, MachineStatus } from "./accountMachines";

export const SECONDS_PER_DAY = 86_400;
export const DEFAULT_STALE_UNREACHABLE_DAYS = 7;

export type StaleCleanupReason = "signed_out" | "membership_expired" | "unreachable";

export interface StaleMachineCandidate {
  installationId: string;
  name: string;
  status: MachineStatus;
  reason: StaleCleanupReason;
  lastSeenAt: number | null;
}

export type RemoveStaleRowStatus =
  | "would_remove"
  | "removed"
  | "already_removed"
  | "refused_self"
  | "refused_online"
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
  rows: RemoveStaleRowResult[];
}

export function classifyStaleRow(
  row: MachineRowView,
  now: number,
  olderThanDays?: number,
): StaleCleanupReason | null {
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

/** Pure selection of stale machines eligible for batch cleanup from the Mesh List view. */
export function selectStaleMachineRows(
  rows: readonly MachineRowView[],
  options: { now?: number; olderThanDays?: number } = {},
): StaleMachineCandidate[] {
  const now = options.now ?? Math.floor(Date.now() / 1000);
  const candidates: StaleMachineCandidate[] = [];
  for (const row of rows) {
    const reason = classifyStaleRow(row, now, options.olderThanDays);
    if (!reason) continue;
    candidates.push({
      installationId: row.key,
      name: row.name,
      status: row.status,
      reason,
      lastSeenAt: row.lastSeenAt,
    });
  }
  candidates.sort((a, b) => {
    const aSeen = a.lastSeenAt ?? 0;
    const bSeen = b.lastSeenAt ?? 0;
    return aSeen - bSeen || a.installationId.localeCompare(b.installationId);
  });
  return candidates;
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
