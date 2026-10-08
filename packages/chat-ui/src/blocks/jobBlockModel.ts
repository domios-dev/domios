import type { ChatBlockTone } from "./ChatBlockFrame";

export type JobKind = "cron" | "blueprint";
export type JobRunState = "enabled" | "paused" | "running" | "stopped";

/** Facts the host reports about one Cron or Blueprint; text fields arrive already formatted. */
export interface JobBlockInfo {
  id: string;
  kind: JobKind;
  name: string;
  state: JobRunState;
  /** The host's own state word, such as a Blueprint's "Finishing". */
  stateLabel: string;
  schedule: string;
  nextRun: string | null;
  lastResult: { label: string; tone: ChatBlockTone } | null;
  canRunNow: boolean;
  canPause: boolean;
  canStop: boolean;
}

export type JobBlockState =
  | { kind: "loading" }
  | { kind: "unavailable"; message: string }
  | { kind: "missing" }
  | { kind: "ready"; job: JobBlockInfo };

const JOB_REF = /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$/;

export function parseJobRef(argument: string): string | null {
  const ref = argument.trim();
  return JOB_REF.test(ref) ? ref : null;
}

export const JOB_STATE_TONE: Record<JobRunState, ChatBlockTone> = {
  enabled: "ok",
  paused: "muted",
  running: "accent",
  stopped: "muted",
};

export const JOB_KIND_LABEL: Record<JobKind, string> = { cron: "Cron", blueprint: "Blueprint" };

export interface JobActionPlan {
  runNow: boolean;
  pause: "pause" | "resume" | null;
  stop: boolean;
}

/** Only actions the host can really perform; a Blueprint without a pause command never shows one. */
export function jobActionPlan(job: JobBlockInfo): JobActionPlan {
  const pause = !job.canPause ? null : job.state === "paused" ? "resume" : "pause";
  return { runNow: job.canRunNow && job.state !== "running", pause, stop: job.canStop && job.state === "running" };
}

export function jobScheduleLine(job: JobBlockInfo): string {
  if (job.state === "paused") return `${job.schedule}, paused`;
  return job.nextRun ? `${job.schedule}, next ${job.nextRun}` : job.schedule;
}
