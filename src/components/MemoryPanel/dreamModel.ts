import type { GraphWorkerInfo } from "../../lib/types";
import { isTerminalWorkerStatus } from "../AgentRun/workerEmptyState";
import { diaryCompleteness } from "./dreamRunStatus";
import { parseDream } from "./dreamVisuals";

export interface DreamDiaryEntry {
  id: string;
  title: string;
  body: string;
  created_at: string;
  area: string | null;
  tags?: string[];
}

export type DreamAction = "linked" | "merged" | "archived" | "reweighted";
export type DreamWorker = GraphWorkerInfo & {
  node_actions?: Record<string, DreamAction>;
  created_edges?: { from: string; to: string; relation: string }[];
  stage?: string | null;
};
export interface DreamCounters {
  links: number;
  merged: number;
  archived: number;
  reweighted: number;
  collapsed: number;
  backlinks: number;
}
export interface DreamNight {
  id: string;
  at: number;
  entry?: DreamDiaryEntry;
  worker?: DreamWorker;
  state: "complete" | "incomplete" | "stopped" | "live";
  counters: DreamCounters;
}

export function dreamCounters(body: string): DreamCounters {
  const log = parseDream(body).log ?? "";
  const fields = new Map(log.split("\n").map((line) => {
    const colon = line.indexOf(":");
    return [line.slice(0, colon).trim(), line.slice(colon + 1).trim()];
  }));
  const amount = (name: string, key?: string) => {
    const text = fields.get(name) ?? "";
    const match = key ? text.match(new RegExp(`\\b${key}=(\\d+)\\b`)) : text.match(/^(\d+)\b/);
    return match ? Number(match[1]) : 0;
  };
  return {
    links: amount("ISLANDS", "bridged") + amount("ORPHANS", "bridged"),
    merged: amount("DEDUP", "aliases") + amount("DEDUP", "duplicates"),
    archived: amount("ISLANDS", "archived") + amount("ORPHANS", "archived") + amount("CALIBRATION", "archived"),
    reweighted: amount("CALIBRATION", "applied"),
    collapsed: amount("PARALLEL_EDGES") || amount("PARALLEL_EDGES", "collapsed"),
    backlinks: amount("DREAM_BACKLINKS") || amount("DREAM_BACKLINKS", "pruned"),
  };
}

export function nightState(entry?: DreamDiaryEntry, worker?: DreamWorker): DreamNight["state"] {
  if (worker && !isTerminalWorkerStatus(worker.status)) return "live";
  if (worker?.execution_end && ["cancelled", "timed_out", "orphaned"].includes(worker.execution_end.kind)) return "stopped";
  if (worker?.finalization?.state === "failed" || (worker && worker.status !== "done")) return "stopped";
  if (!entry || !diaryCompleteness(entry.body).complete || worker?.finalization?.state === "partial") return "incomplete";
  return "complete";
}

export function stoppedDreamCopy(worker?: DreamWorker) {
  const duration = worker?.done_at == null ? null : worker.done_at - worker.spawned_at;
  const headline = !worker?.stage || duration === 0
    ? "This night didn't run."
    : "This night didn't finish.";
  const rawReason = worker?.finalization && "reason" in worker.finalization
    ? worker.finalization.reason : worker?.report;
  const kind = worker?.execution_end?.kind;
  const reason = kind === "cancelled" || /\bworker cancel(?:led|ed)\b/i.test(rawReason ?? "")
    ? "Stopped by you"
    : kind === "timed_out" ? "The run timed out"
      : kind === "orphaned" ? "The run lost contact"
        : rawReason?.trim() || "The run stopped";
  return { headline, reason };
}

function nightCounters(body: string, worker?: DreamWorker): DreamCounters {
  const counters = dreamCounters(body);
  return worker?.created_edges?.length
    ? { ...counters, links: dreamRunData(worker).edges.length }
    : counters;
}

export function dreamNights(entries: DreamDiaryEntry[], workers: DreamWorker[]): DreamNight[] {
  const ordered = [...workers].sort((a, b) => b.spawned_at - a.spawned_at);
  const represented = new Set(entries.map((entry) => entry.id));
  const nights = entries.map((entry): DreamNight => {
    const worker = ordered.find((run) => run.diary_id === entry.id);
    return { id: entry.id, at: worker?.spawned_at ?? Date.parse(entry.created_at), entry, worker,
      state: nightState(entry, worker), counters: nightCounters(entry.body, worker) };
  });
  for (const worker of ordered) {
    if (worker.diary_id && represented.has(worker.diary_id)) continue;
    if (worker.diary_id) represented.add(worker.diary_id);
    nights.push({ id: worker.worker_id, at: worker.spawned_at, worker,
      state: nightState(undefined, worker), counters: nightCounters("", worker) });
  }
  return nights.filter((night) => Number.isFinite(night.at)).sort((a, b) => b.at - a.at);
}

export function dreamDay(at: number): string {
  const date = new Date(at);
  return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;
}

export function dreamHeatmap(nights: DreamNight[], now: number) {
  const end = new Date(now);
  end.setHours(0, 0, 0, 0);
  const start = new Date(end);
  start.setDate(start.getDate() - end.getDay() - 17 * 7);
  const byDay = new Map<string, DreamNight[]>();
  for (const night of nights) {
    if (night.state === "live") continue;
    const key = dreamDay(night.at);
    byDay.set(key, [...(byDay.get(key) ?? []), night]);
  }
  return Array.from({ length: 18 * 7 }, (_, index) => {
    const date = new Date(start);
    date.setDate(date.getDate() + index);
    const runs = byDay.get(dreamDay(date.getTime())) ?? [];
    return { at: date.getTime(), nights: date > end ? [] : runs, future: date > end, today: +date === +end };
  });
}

export function dreamTotals(nights: DreamNight[], now: number) {
  const cells = dreamHeatmap(nights, now).filter((cell) => !cell.future);
  const runs = cells.flatMap((cell) => cell.nights);
  return {
    nights: cells.filter((cell) => cell.nights.some((night) => night.entry)).length,
    links: runs.reduce((sum, night) => sum + night.counters.links, 0),
    merged: runs.reduce((sum, night) => sum + night.counters.merged, 0),
    stopped: cells.filter((cell) => cell.nights.some((night) => night.state === "stopped")).length,
  };
}

export function selectDreamNight(nights: DreamNight[], selected: string | null): DreamNight | undefined {
  return nights.find((night) => night.id === selected) ?? nights.find((night) => night.state === "complete") ?? nights[0];
}

export function dreamRunData(worker?: DreamWorker) {
  const diaryId = worker?.diary_id;
  const created = worker?.created_edges ?? [];
  const realEdges = created.filter((edge) => edge.from !== diaryId && edge.to !== diaryId);
  const realEndpoints = new Set(realEdges.flatMap((edge) => [edge.from, edge.to]));
  const diaryEndpoints = new Set(created.filter((edge) => edge.from === diaryId || edge.to === diaryId)
    .flatMap((edge) => [edge.from, edge.to]));
  const ids = [...new Set(worker?.touched_nodes ?? [])].filter((id) => {
    if (id === diaryId) return false;
    const action = worker?.node_actions?.[id];
    return !diaryEndpoints.has(id) || realEndpoints.has(id) || (action !== undefined && action !== "linked");
  });
  const idSet = new Set(ids);
  const edges = realEdges.filter((edge) => idSet.has(edge.from) && idSet.has(edge.to));
  const actions = Object.entries(worker?.node_actions ?? {}).filter(([id]) => idSet.has(id));
  return { ids, edges, actions, hasConstellation: ids.length > 0, hasItems: actions.length > 0 };
}

export const DREAM_STAGES = [
  ["read_memories", "Read memories"],
  ["prune_backlinks", "Prune diary backlinks"],
  ["bridge_islands", "Bridging islands"],
  ["check_duplicates", "Check duplicates"],
  ["write_diary", "Write the diary"],
] as const;
