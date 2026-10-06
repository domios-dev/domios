import type { CronJob } from "../../lib/types";
import { ALL_JOBS_SCOPE } from "../Automation/types";
import { sentinelTitle, type SentinelJob } from "../Automation/scenarios";

export const ALL_JOBS = ALL_JOBS_SCOPE;

/** A null field is an absent capsule: no filter on that axis. */
export interface JobsFilter {
  workspaceId: string | null;
  teamId: string | null;
}

export const NO_JOBS_FILTER: JobsFilter = { workspaceId: null, teamId: null };

export type MergedJob =
  | { kind: "cron"; cron: CronJob; title: string; active: boolean; state: string }
  | { kind: "sentinel"; sentinel: SentinelJob; title: string; active: boolean; state: string };

export function cronState(job: CronJob): string {
  if (job.last_run && (job.last_run.status === "Running" || job.last_run.status === "AwaitingOwnerPrompt")) return "Running";
  return job.enabled ? "Enabled" : "Paused";
}

export function matchesJobsFilter(filter: JobsFilter, workspaceId: string, teamId: string | null | undefined): boolean {
  return (filter.workspaceId === null || workspaceId === filter.workspaceId)
    && (filter.teamId === null || (teamId ?? null) === filter.teamId);
}

export function mergeJobs(filter: JobsFilter, crons: readonly CronJob[], sentinels: readonly SentinelJob[]): MergedJob[] {
  const jobs: MergedJob[] = [
    ...crons.filter((job) => matchesJobsFilter(filter, job.workspace_id, job.team_id)).map((cron): MergedJob => ({
      kind: "cron",
      cron,
      title: cron.name,
      active: cron.enabled || cronState(cron) === "Running",
      state: cronState(cron),
    })),
    ...sentinels.filter((job) => matchesJobsFilter(filter, job.scenario.root.workspace_id, job.scenario.root.team_id)).map((sentinel): MergedJob => ({
      kind: "sentinel",
      sentinel,
      title: sentinelTitle(sentinel.scenario, sentinel.root),
      active: ["active", "running", "finishing"].includes(sentinel.state.toLowerCase()),
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

export interface JobsIndicator {
  count: number;
  running: number;
}

/** A running Cron run or a live Blueprint graph; an enabled Cron waiting for its schedule is not running. */
export function isJobRunning(job: MergedJob): boolean {
  return job.kind === "cron" ? job.state === "Running" : job.active;
}

export function jobsIndicator(jobs: { crons: readonly CronJob[]; sentinels: readonly SentinelJob[] }, scope: JobsFilter): JobsIndicator {
  const merged = mergeJobs(scope, jobs.crons, jobs.sentinels);
  return { count: merged.length, running: merged.filter(isJobRunning).length };
}

export function jobsAccessibleName({ count, running }: JobsIndicator): string {
  if (count === 0) return "Jobs";
  return running > 0 ? `Jobs, ${count}, ${running} running` : `Jobs, ${count}`;
}
