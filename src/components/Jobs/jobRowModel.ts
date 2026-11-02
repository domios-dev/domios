import { blueprintMeta } from "../Automation/blueprintMeta";
import { probeStateLabel } from "../Automation/probeProgress";
import { describeSchedule, formatFuture } from "../../lib/formatters";
import { formatDuration } from "../../lib/timeLabels";
import type { CronJob } from "../../lib/types";
import type { BlueprintScenarioJob } from "../Automation/scenarios";
export function cronMeta(job: CronJob): string {
  const schedule = describeSchedule(job.schedule);
  const next = job.enabled && job.next_run ? `${schedule} · next ${formatFuture(job.next_run)}` : schedule;
  return `${next} · last run: ${job.last_run?.status ?? "none"}`;
}

/** A Blueprint row's secondary line describes what it does; ids live only in the hover hint. */
export function sentinelRowText(job: BlueprintScenarioJob, title: string, now = Date.now()): { meta: string; idHint: string } {
  const { root, scenario } = job;
  const purpose = root?.purpose?.trim();
  const schedule = root?.schedule?.trim();
  const meta = purpose && purpose !== title ? purpose
    : schedule ? describeSchedule(schedule)
    : root?.cadence_ms ? `Every ${formatDuration(root.cadence_ms)}`
    : "No description";
  return { meta: blueprintMeta(root?.note || meta, root?.state, root?.deletes_at, now), idHint: `Blueprint ${scenario.id}` };
}

export function sentinelStateLabel(job: BlueprintScenarioJob): string {
  return job.root ? probeStateLabel(job.root) : job.state;
}
