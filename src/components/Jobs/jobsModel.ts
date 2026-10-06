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

/** A running Cron run or a live Sentinel graph; an enabled Cron waiting for its schedule is not running. */
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

export function canStopJob(job: MergedJob): boolean {
  return job.kind === "cron"
    ? cronState(job.cron) === "Running"
    : ["active", "running", "finishing"].includes(job.state.toLowerCase()) && Boolean(job.sentinel.root?.stop.allowed);
}

export type TabJobState = "waiting on you" | "running" | "scheduled" | "stopped";

export interface TabJob {
  id: string;
  title: string;
  kind: MergedJob["kind"];
  state: TabJobState;
}

const TAB_JOB_STATE_ORDER: readonly TabJobState[] = ["waiting on you", "running", "scheduled", "stopped"];

/** Creator, report target, and the latest run's tab bind a Cron; creator, owner, and tab handler targets bind a Sentinel. */
function boundTabIds(job: MergedJob): (string | null | undefined)[] {
  if (job.kind === "cron") return [job.cron.creator_tab_id, job.cron.report_target_tab_id, job.cron.last_run?.tab_id];
  const { scenario, root } = job.sentinel;
  const targets = root?.outcomes.flatMap((outcome) => outcome.handlers.map((handler) => handler.target?.kind === "worker" ? handler.target.id : null)) ?? [];
  return [scenario.root.owner_tab_id, root?.identity.owner_tab_id, root?.sentinel_definition?.creator_tab_id, ...targets];
}

export function tabJobState(job: MergedJob): TabJobState {
  if (job.kind === "cron" && job.cron.last_run?.status === "AwaitingOwnerPrompt") return "waiting on you";
  if (isJobRunning(job)) return "running";
  return job.kind === "cron" && job.cron.enabled ? "scheduled" : "stopped";
}

export function tabJobKey(job: Pick<TabJob, "kind" | "id">): string {
  return `${job.kind}:${job.id}`;
}

export function mergedJobId(job: MergedJob): string {
  return job.kind === "cron" ? job.cron.id : job.sentinel.scenario.id;
}

export function jobsForTab(jobs: readonly MergedJob[], tabId: string): TabJob[] {
  return jobs
    .filter((job) => boundTabIds(job).includes(tabId))
    .map((job): TabJob => ({ id: mergedJobId(job), title: job.title, kind: job.kind, state: tabJobState(job) }))
    .sort((a, b) => TAB_JOB_STATE_ORDER.indexOf(a.state) - TAB_JOB_STATE_ORDER.indexOf(b.state) || a.title.localeCompare(b.title));
}

export function tabJobAccessibleName(job: TabJob): string {
  return `Job ${job.title}, ${job.state}`;
}
