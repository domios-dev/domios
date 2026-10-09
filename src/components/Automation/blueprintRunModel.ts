import { decisionKind } from "./nodeDecision";
import { automationKey, type AutomationGraph, type AutomationTransition, type AutomationNode } from "./types";
import type { GraphRun, RunEvent } from "../../lib/blueprintRun";
import { blueprintModel, nodeKeys, wireId, type BlueprintModel, type BlueprintNode, type BlueprintWire } from "./blueprintModel";

/** One hop of a replay: what lights up together as execution reaches the next node. */
export interface ReplayStep { nodes: string[]; wires: string[] }

export interface RunTransition {
  id: string;
  stage: string;
  bucket: string;
  at: number;
  events: RunEvent[];
}

export type RunStatus = "running" | "completed" | "failed" | "unconfirmed";

export interface RunRow {
  key: string;
  at: number;
  status: RunStatus;
  trigger: string;
  path: string[];
  confidence: number | null;
  durationMs: number | null;
  run: GraphRun;
}

export interface TimelineEntry {
  kind: "trigger" | "source" | "decision" | "action";
  title: string;
  detail: string | null;
  status: string | null;
  at: number;
}

export const REPLAY_STEP_MS = 520;
const FAILED = new Set(["failed", "error", "deadline", "cancelled", "nonzero"]);
const QUIET = new Set(["skipped", "noop", "No action"]);

/** Groups the events of each stage decision in first-seen order; events without a stage reference stay out of the typed path. */
export function runTransitions(run: GraphRun): RunTransition[] {
  const transitions = new Map<string, RunTransition>();
  for (const event of run.events) {
    if (!event.stage) continue;
    const id = JSON.stringify([event.stage, event.transition ?? event.at]);
    const known = transitions.get(id);
    if (known) known.events.push(event);
    else transitions.set(id, { id, stage: event.stage, bucket: event.outcome, at: event.at, events: [event] });
  }
  return [...transitions.values()];
}

export function replaySteps(model: BlueprintModel, run: GraphRun): ReplayStep[] {
  const nodes = new Set(model.nodes.map((node) => node.key));
  const wires = new Set(model.wires.map((wire) => wire.id));
  const steps: ReplayStep[] = [];
  let previousActions: string[] = [];
  const keep = (step: ReplayStep) => {
    const kept = { nodes: step.nodes.filter((key) => nodes.has(key)), wires: step.wires.filter((id) => wires.has(id)) };
    if (kept.nodes.length || kept.wires.length) steps.push(kept);
  };
  for (const transition of runTransitions(run)) {
    const keys = nodeKeys(transition.stage);
    const bucketPin = `bucket:${transition.bucket}`;
    const chain = previousActions.map((action) => wireId(action, "then", keys.trigger)).filter((id) => wires.has(id));
    keep({ nodes: [keys.trigger], wires: chain });
    keep({ nodes: [keys.source], wires: [wireId(keys.trigger, "then", keys.source)] });
    keep({ nodes: [keys.decision], wires: [wireId(keys.source, "then", keys.decision), wireId(keys.source, "output", keys.decision)] });
    const actions = transition.events.filter((event) => event.handler && event.edgeId !== null && !QUIET.has(event.status)).map((event) => keys.action(transition.bucket, event.handler!));
    keep({ nodes: actions, wires: actions.map((action) => wireId(keys.decision, bucketPin, action)) });
    previousActions = actions;
  }
  return steps;
}

export interface ReplayFrame {
  nodes: Set<string>;
  wires: Set<string>;
  /** Wires the execution pulse is travelling along right now. */
  travelling: string[];
  /** 0..1 along the travelling wires. */
  progress: number;
  /** 0..1 of the whole replay, for the progress bar. */
  overall: number;
  done: boolean;
}

export function replayFrame(steps: readonly ReplayStep[], elapsedMs: number, reducedMotion: boolean, stepMs = REPLAY_STEP_MS): ReplayFrame {
  const total = steps.length * stepMs;
  const elapsed = reducedMotion ? total : Math.max(0, elapsedMs);
  const reached = Math.min(steps.length, Math.floor(elapsed / stepMs) + 1);
  const shown = steps.slice(0, reached);
  const done = reducedMotion || elapsed >= total;
  return {
    nodes: new Set(done ? steps.flatMap((step) => step.nodes) : shown.slice(0, -1).flatMap((step) => step.nodes)),
    wires: new Set(shown.flatMap((step) => step.wires)),
    travelling: done ? [] : steps[reached - 1]?.wires ?? [],
    progress: done ? 1 : (elapsed % stepMs) / stepMs,
    overall: total ? Math.min(1, elapsed / total) : 1,
    done,
  };
}

export function runStatus(run: GraphRun): RunStatus {
  if (run.state === "running") return "running";
  if (run.events.some((event) => FAILED.has(event.status.toLowerCase()))) return "failed";
  if (run.state === "unconfirmed") return "unconfirmed";
  return "completed";
}

function node(model: BlueprintModel, key: string): BlueprintNode | undefined {
  return model.nodes.find((entry) => entry.key === key);
}

function decisionName(event: RunEvent | undefined) {
  if (event?.decidedBy === "pattern") return "Pattern rules";
  return event?.classifier?.label ?? "Decision origin unknown";
}

export function runRow(model: BlueprintModel, run: GraphRun): RunRow {
  const transitions = runTransitions(run);
  const first = transitions[0];
  const path: string[] = [];
  if (first) {
    path.push(node(model, nodeKeys(first.stage).source)?.title ?? first.events[0].node);
    path.push(`${decisionName(first.events[0])}: ${first.bucket}`);
    const actions = first.events.filter((event) => event.handler && !QUIET.has(event.status)).map((event) => node(model, nodeKeys(first.stage).action(first.bucket, event.handler!))?.title ?? event.target ?? event.handler!);
    path.push(actions.length ? actions.join(", ") : "no action");
    if (transitions.length > 1) path.push(`+${transitions.length - 1} more ${transitions.length === 2 ? "stage" : "stages"}`);
  }
  const ends = run.events.map((event) => event.at + (event.duration ?? 0));
  const end = ends.length ? Math.max(...ends) : run.at;
  const confidence = run.events.find((event) => event.confidence != null)?.confidence ?? null;
  return {
    key: run.key, at: run.at, status: runStatus(run), run,
    trigger: first ? node(model, nodeKeys(first.stage).trigger)?.title ?? "Trigger" : "Recorded event",
    path, confidence, durationMs: end > run.at ? end - run.at : null,
  };
}

export function runTimeline(model: BlueprintModel, run: GraphRun): TimelineEntry[] {
  return runTransitions(run).flatMap((transition) => {
    const keys = nodeKeys(transition.stage);
    const head = transition.events[0];
    const trigger = node(model, keys.trigger);
    const source = node(model, keys.source);
    const confidence = head.confidence != null ? ` · ${Math.round(head.confidence * 100)}% confident` : "";
    const decider = decisionName(head);
    const entries: TimelineEntry[] = [
      { kind: "trigger", title: trigger?.title ?? head.node, detail: head.node, status: null, at: transition.at },
      { kind: "source", title: source?.title ?? "Source", detail: source?.subtitle ?? null, status: null, at: transition.at },
      { kind: "decision", title: `${decider} chose ${transition.bucket}${confidence}`, detail: head.reason ?? null, status: null, at: transition.at },
    ];
    for (const event of transition.events) {
      if (!event.handler) { entries.push({ kind: "action", title: "No action declared for this route", detail: null, status: event.status, at: event.at }); continue; }
      const action = node(model, keys.action(transition.bucket, event.handler));
      entries.push({ kind: "action", title: action?.title ?? event.target ?? event.handler, detail: event.duration === null ? null : formatDuration(event.duration), status: event.status, at: event.at });
    }
    return entries;
  });
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 1 : 0)} s`;
  return `${Math.floor(ms / 60_000)} min ${Math.round((ms % 60_000) / 1000)} s`;
}

export function timelineText(row: RunRow, entries: readonly TimelineEntry[], time: (at: number) => string): string {
  const head = `${time(row.at)} ${row.status} · ${row.trigger} · ${row.path.join(" → ")}${row.confidence != null ? ` · ${Math.round(row.confidence * 100)}%` : ""}${row.durationMs != null ? ` · ${formatDuration(row.durationMs)}` : ""}`;
  return [head, ...entries.map((entry) => `  ${entry.kind}: ${entry.title}${entry.status ? ` (${entry.status})` : ""}${entry.detail ? ` — ${entry.detail}` : ""}`)].join("\n");
}

/** Arrow keys walk the run list without wrapping; Home and End jump to the ends. */
export function nextRunIndex(current: number, count: number, code: string): number | null {
  if (!count) return null;
  if (code === "ArrowDown") return Math.min(count - 1, current + 1);
  if (code === "ArrowUp") return Math.max(0, current - 1);
  if (code === "Home") return 0;
  if (code === "End") return count - 1;
  return null;
}

const routeKey = (source: string, bucket: string, handler: string | null) => JSON.stringify([source, bucket, handler]);

function transitionKey(node: AutomationNode, transition: AutomationTransition) {
  return JSON.stringify([automationKey(node.identity), transition.id]);
}

export function graphRuns(graph: AutomationGraph): GraphRun[] {
  const model = blueprintModel(graph);
  const names = new Map(model.nodes.map((node) => [node.key, node.action?.target?.name ?? node.action?.handler?.label ?? node.title]));
  const pieces = graph.nodes.flatMap((node) => [...(node.history ?? [])].reverse().map((transition) => ({ node, transition, key: transitionKey(node, transition) })));
  const byTransition = new Map(pieces.map((piece) => [piece.key, piece]));
  const roots = new Map(graph.nodes.filter((node) => node.identity.kind === "blueprint").map((node) => [node.identity.id, node]));
  const routes = new Map<string, BlueprintWire>();
  for (const edge of model.wires) {
    if (!edge.handlerId || !edge.bucket || edge.back || edge.from.node !== nodeKeys(edge.stageKey).decision) continue;
    const key = routeKey(edge.stageKey, edge.bucket, edge.handlerId);
    if (!routes.has(key)) routes.set(key, edge);
  }
  const eventsFor = ({ node, transition }: typeof pieces[number]): RunEvent[] => {
    const source = automationKey(node.identity);
    const events: RunEvent[] = transition.handlers.map((handler) => {
      const edge = transition.routes_current === false ? undefined : routes.get(routeKey(source, transition.bucket, handler.handler_id));
      const command = handler.command_result;
      const duration = command && Number.isSafeInteger(command.started_at) && Number.isSafeInteger(command.finished_at) && command.started_at >= 0 && command.finished_at >= command.started_at ? command.finished_at - command.started_at : null;
      return {
        decidedBy: transition.decided_by, classifier: decisionKind(transition), confidence: transition.confidence, reason: transition.reason,
        at: handler.accepted_at ?? transition.accepted_at, node: node.name, outcome: transition.bucket,
        target: edge ? names.get(edge.to.node) ?? null : handler.handler_id,
        status: handler.status ?? "Unknown", duration, stage: source, transition: transition.id, handler: handler.handler_id,
        edgeId: handler.status === "skipped" || handler.status === "noop" ? null : edge?.id ?? null,
      };
    });
    if (!events.length) events.push({ decidedBy: transition.decided_by, classifier: decisionKind(transition), confidence: transition.confidence, reason: transition.reason, at: transition.accepted_at, node: node.name, outcome: transition.bucket, target: null, status: "No action", duration: null, edgeId: null, stage: source, transition: transition.id, handler: null });
    return events;
  };
  const runs: GraphRun[] = (graph.runs ?? []).flatMap((run) => {
    const root = roots.get(run.root_node);
    if (!root) return [];
    const entries = run.transitions.flatMap((entry) => {
      const piece = byTransition.get(JSON.stringify([automationKey(entry.node), entry.transition_id]));
      return piece ? [{ ...piece, parent: entry.parent }] : [];
    });
    const children = new Map<string, typeof entries>();
    for (const entry of entries) {
      if (!entry.parent) continue;
      const key = routeKey(entry.parent.node, entry.parent.transition, entry.parent.handler);
      const siblings = children.get(key) ?? [];
      siblings.push(entry);
      children.set(key, siblings);
    }
    const visited = new Set<string>();
    const events: RunEvent[] = [];
    const visit = (entry: typeof entries[number]) => {
      if (visited.has(entry.key)) return;
      visited.add(entry.key);
      eventsFor(entry).forEach((event, index) => {
        events.push(event);
        const handler = entry.transition.handlers[index];
        if (handler) children.get(routeKey(entry.node.identity.id, entry.transition.id, handler.handler_id))?.forEach(visit);
      });
    };
    entries.filter((entry) => entry.parent === null).forEach(visit);
    entries.forEach(visit);
    return [{ key: run.id, at: run.started_at, name: root.name, events, state: run.state, conversation: root.conversation, incomplete: run.incomplete || entries.length < run.transitions.length }];
  });
  for (const piece of pieces) {
    if (piece.transition.causal_run) continue;
    runs.push({ key: piece.key, at: piece.transition.accepted_at, name: piece.node.name, events: eventsFor(piece), state: "unlinked", conversation: piece.node.conversation, incomplete: false });
  }
  return runs.sort((a, b) => b.at - a.at || a.key.localeCompare(b.key));
}
