import type { CronJob } from "../../lib/types";
import { ALL_JOBS_SCOPE } from "../Automation/types";
import { sentinelTitle, type SentinelJob } from "../Automation/scenarios";

export const ALL_JOBS = ALL_JOBS_SCOPE;
export type JobsTeamFilter = string | null | typeof ALL_JOBS;

export type MergedJob =
  | { kind: "cron"; cron: CronJob; title: string; active: boolean; state: string }
  | { kind: "sentinel"; sentinel: SentinelJob; title: string; active: boolean; state: string };

export function cronState(job: CronJob): string {
  if (job.last_run && (job.last_run.status === "Running" || job.last_run.status === "AwaitingOwnerPrompt")) return "Running";
  return job.enabled ? "Enabled" : "Paused";
}

export function mergeJobs(
  workspaceId: string,
  teamFilter: JobsTeamFilter,
  crons: readonly CronJob[],
  sentinels: readonly SentinelJob[],
): MergedJob[] {
  const matchesTeam = (teamId: string | null | undefined) => teamFilter === ALL_JOBS || (teamId ?? null) === teamFilter;
  const jobs: MergedJob[] = [
    ...crons.filter((job) => job.workspace_id === workspaceId && matchesTeam(job.team_id)).map((cron): MergedJob => ({
      kind: "cron",
      cron,
      title: cron.name,
      active: cron.enabled || cronState(cron) === "Running",
      state: cronState(cron),
    })),
    ...sentinels.filter((job) => job.scenario.root.workspace_id === workspaceId && matchesTeam(job.scenario.root.team_id)).map((sentinel): MergedJob => ({
      kind: "sentinel",
      sentinel,
      title: sentinelTitle(sentinel.scenario, sentinel.root),
      active: sentinel.state.toLowerCase() === "active" || sentinel.state.toLowerCase() === "finishing",
      state: sentinel.state,
    })),
  ];
  return jobs.sort((a, b) => Number(b.active) - Number(a.active) || a.title.localeCompare(b.title));
}

export function clearableJobs(jobs: readonly MergedJob[]): MergedJob[] {
  return jobs.filter((job) => job.kind === "cron"
    ? !job.cron.enabled && cronState(job.cron) !== "Running"
    : ["stopped", "expired", "finished"].includes(job.state.toLowerCase()));
}

export function activeJobsCount(workspaceId: string, crons: readonly CronJob[], sentinels: readonly SentinelJob[]): number {
  return mergeJobs(workspaceId, ALL_JOBS, crons, sentinels).filter((job) => job.active).length;
}
