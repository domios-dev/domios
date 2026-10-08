import type { ClosedStructureView } from "./types";

export function restorableStructures(records: readonly ClosedStructureView[]) {
  return records.filter((record) => record.state !== "restored");
}

export function undoableClose(records: readonly ClosedStructureView[], now: number) {
  const latest = records.reduce<ClosedStructureView | null>((found, record) =>
    !found || record.closed_at > found.closed_at ? record : found, null);
  return latest?.state === "closed" && now <= latest.undo_until ? latest : null;
}

export function describeRestore(record: ClosedStructureView): string {
  const parts = [`${record.member_count} tabs`, `${record.agent_count} agents`];
  if (record.team_count) parts.push(`${record.team_count} Teams`);
  if (record.shell_count) parts.push(`${record.shell_count} shells (stopped)`);
  if (record.cron_count) parts.push(`${record.cron_count} crons`);
  if (record.blueprint_count) parts.push(`${record.blueprint_count} Blueprints`);
  return parts.join(" · ");
}
