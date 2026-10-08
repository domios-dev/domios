import type { ChatBlockTone, JobBlockInfo, JobBlockState, JobRunState } from "@codeterm/chat-ui";
import { blueprintJobs, cronState, type BlueprintJob } from "../../components/Jobs/jobsModel";
import type { BlueprintSnapshot } from "../../components/Automation/types";
import { describeSchedule, formatFuture, timeAgo } from "../formatters";
import type { CronJob, CronRunStatus } from "../types";
import type { JobsSnapshot } from "./jobsBlockStore";

export type ResolvedJob =
  | { kind: "cron"; cron: CronJob }
  | { kind: "blueprint"; workspaceId: string; job: BlueprintJob; snapshot: BlueprintSnapshot };

function unique<T>(matches: readonly T[]): T | null {
  return matches.length === 1 ? matches[0] : null;
}

/** An exact id wins; an exact name counts only when it names exactly one job. */
export function resolveJob(snapshot: JobsSnapshot, ref: string): ResolvedJob | null {
  const crons = snapshot.crons ?? [];
  const blueprints = (snapshot.graphs ?? []).flatMap(({ workspaceId, graph }) => blueprintJobs(graph).flatMap((job): ResolvedJob[] => {
    const blueprint = graph.blueprints?.find((entry) => entry.id === job.pipeline.id);
    return blueprint ? [{ kind: "blueprint", workspaceId, job, snapshot: blueprint }] : [];
  }));
  const cronById = crons.find((cron) => cron.id === ref);
  if (cronById) return { kind: "cron", cron: cronById };
  const blueprintById = blueprints.find((entry) => entry.kind === "blueprint" && entry.snapshot.id === ref);
  if (blueprintById) return blueprintById;
  return unique([
    ...crons.filter((cron) => cron.name === ref).map((cron): ResolvedJob => ({ kind: "cron", cron })),
    ...blueprints.filter((entry) => entry.kind === "blueprint" && entry.snapshot.name === ref),
  ]);
}

const RUN_RESULT: Record<CronRunStatus, { label: string; tone: ChatBlockTone } | null> = {
  Running: null,
  AwaitingOwnerPrompt: { label: "Waiting for the owner", tone: "warn" },
  Success: { label: "Succeeded", tone: "ok" },
  Failed: { label: "Failed", tone: "warn" },
  Cancelled: { label: "Cancelled", tone: "muted" },
};

export function cronBlockInfo(cron: CronJob, now = Date.now()): JobBlockInfo {
  const label = cronState(cron);
  const state: JobRunState = label === "Running" ? "running" : cron.enabled ? "enabled" : "paused";
  const result = cron.last_run ? RUN_RESULT[cron.last_run.status] : null;
  const at = cron.last_run ? cron.last_run.finished_at ?? cron.last_run.started_at : null;
  return {
    id: cron.id,
    kind: "cron",
    name: cron.name,
    state,
    stateLabel: label,
    schedule: cron.oneshot ? `Once, ${describeSchedule(cron.schedule)}` : describeSchedule(cron.schedule),
    nextRun: cron.enabled && cron.next_run && cron.next_run > now ? formatFuture(cron.next_run) : null,
    lastResult: result && at ? { label: `${result.label} ${timeAgo(at)}`, tone: result.tone } : result,
    canRunNow: true,
    canPause: true,
    canStop: true,
  };
}

const BLUEPRINT_STATE: Record<string, JobRunState> = { running: "running", stopped: "stopped", failed: "stopped" };

export function blueprintBlockInfo(entry: Extract<ResolvedJob, { kind: "blueprint" }>): JobBlockInfo {
  const { snapshot, job } = entry;
  const result = snapshot.result?.state;
  const at = snapshot.last_event_at_ms;
  const lastResult = result === "delivered" ? { label: "Delivered", tone: "ok" as const }
    : result === "failed" ? { label: snapshot.result?.reason ? `Failed: ${snapshot.result.reason}` : "Failed", tone: "warn" as const }
    : null;
  return {
    id: snapshot.id,
    kind: "blueprint",
    name: snapshot.name,
    state: BLUEPRINT_STATE[snapshot.state] ?? "enabled",
    stateLabel: snapshot.state.charAt(0).toUpperCase() + snapshot.state.slice(1),
    schedule: snapshot.trigger?.summary?.trim() || "No trigger",
    nextRun: null,
    lastResult: lastResult && at ? { ...lastResult, label: `${lastResult.label} ${timeAgo(at)}` } : lastResult,
    canRunNow: false,
    canPause: false,
    canStop: job.stopAllowed,
  };
}

/** Honest card state: a job missing from a list that failed to load is unavailable, never "not found". */
export function jobBlockState(snapshot: JobsSnapshot, ref: string, now = Date.now()): JobBlockState {
  const resolved = resolveJob(snapshot, ref);
  if (resolved) return { kind: "ready", job: resolved.kind === "cron" ? cronBlockInfo(resolved.cron, now) : blueprintBlockInfo(resolved) };
  const error = snapshot.cronsError ?? snapshot.graphsError;
  if (error) return { kind: "unavailable", message: error };
  if (snapshot.crons === null || snapshot.graphs === null) return { kind: "loading" };
  return { kind: "missing" };
}
