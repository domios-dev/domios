import { litWires, type BlueprintGraphModel, type BlueprintRunEvent, type NodeEvidence } from "./graphModel";

export interface ReplayState { step: number; playing: boolean }
export type ReplayAction = { type: "play" | "pause" | "advance" } | { type: "seek"; step: number };

export function replayStep(state: ReplayState, action: ReplayAction, total: number): ReplayState {
  const step = Math.max(0, Math.min(total, Math.trunc(state.step)));
  if (action.type === "pause") return { step, playing: false };
  if (action.type === "seek") return { step: Math.max(0, Math.min(total, Math.trunc(action.step))), playing: false };
  if (action.type === "play") return { step: step === total ? 0 : step, playing: total > 0 };
  if (!state.playing) return { step, playing: false };
  const next = Math.min(total, step + 1);
  return { step: next, playing: next < total };
}

export function projectRunPath(model: BlueprintGraphModel, events: readonly BlueprintRunEvent[], historical: boolean, step = events.length) {
  if (!historical || model.mode !== "run" || !events.length) return { model, nodes: new Set<string>(), wires: new Set<string>(), active: null, dim: false };
  const through = Math.max(0, Math.min(events.length, Math.trunc(step)));
  const seen = new Map<string, NodeEvidence>();
  const traversed = new Map<string, Set<string>>();
  for (const event of events.slice(0, through)) {
    seen.set(event.key, event.evidence);
    if (["not reached", "pending", "skipped"].includes(event.evidence.state)) continue;
    const outcomes = traversed.get(event.key) ?? new Set<string>();
    if (event.evidence.outcome) outcomes.add(event.evidence.outcome);
    traversed.set(event.key, outcomes);
  }
  for (const node of model.nodes) if (through && node.control === "trigger" && node.evidence && !seen.has(node.key)) {
    seen.set(node.key, node.evidence);
    traversed.set(node.key, new Set());
  }
  const projected = { ...model, nodes: model.nodes.map((node) => ({ ...node, evidence: seen.get(node.key) ?? { state: "not reached", outcome: null, summary: null, at: null } })) };
  const nodes = new Set(traversed.keys());
  const wires = litWires(projected);
  for (const wire of model.wires) if (nodes.has(wire.from.node) && nodes.has(wire.to.node)
    && (wire.kind === "data" || wire.outcome === null || traversed.get(wire.from.node)?.has(wire.outcome))) wires.add(wire.id);
  const current = events[through - 1]?.key ?? null;
  const keys = new Set(model.nodes.map((node) => node.key));
  let active = current;
  if (active && !keys.has(active)) {
    const path = JSON.parse(active) as string[];
    while (path.length && !keys.has(JSON.stringify(path))) path.pop();
    active = path.length ? JSON.stringify(path) : null;
  }
  return { model: projected, nodes, wires, active, dim: true };
}
