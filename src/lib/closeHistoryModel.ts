import type { ClosedStructureView } from "./types";

export function restorableStructures(records: readonly ClosedStructureView[]) {
  return records.filter((record) => record.state !== "restored");
}

function count(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** A closed record's display name: an unnamed Team shows "Team", never its raw id. */
export function closedLabel(record: ClosedStructureView): string {
  if (record.kind === "team" && /^team_[0-9a-f]+$/i.test(record.label)) return "Team";
  return record.label;
}

export function describeRestore(record: ClosedStructureView): string {
  const parts = [count(record.member_count, "tab", "tabs"), count(record.agent_count, "agent", "agents")];
  if (record.kind === "workspace" && record.team_count) parts.push(count(record.team_count, "Team", "Teams"));
  if (record.shell_count) parts.push(`${count(record.shell_count, "shell", "shells")} (stopped)`);
  if (record.cron_count) parts.push(count(record.cron_count, "cron", "crons"));
  if (record.blueprint_count) parts.push(count(record.blueprint_count, "Blueprint", "Blueprints"));
  return parts.join(" · ");
}
