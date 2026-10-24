import type { BlueprintDocument, BlueprintExpandedCall, BlueprintIrNode, BlueprintJournalNode, BlueprintProjection, BlueprintValue } from "../../lib/blueprintApi";
import type { PipelineView, PipelineViewEdge, PipelineViewNode } from "./pipelineModel";
import { blueprintNodeKey, type BlueprintExpansion } from "./blueprintExpansion";
import { blueprintJournalNodes } from "./blueprintPipelineOutcome";

export interface BlueprintGraphModel { nodes: readonly PipelineViewNode[]; edges: readonly PipelineViewEdge[] }
const baseNode = (key: string, title: string, kind: PipelineViewNode["kind"]): PipelineViewNode => ({
  key, title, kind, state: "waiting", current: false, summary: "", lastEventAtMs: null, resultSummary: null, outcome: null, targetSummary: null,
});

const CONTROL_ROWS: Readonly<Record<string, string>> = { $run: "$trigger", $result: "$result", $stop: "$stop" };

export function nodeLabel(node: PipelineViewNode): string {
  const path = node.callPath ?? [];
  return node.definition ? `${node.definition.id}${path.length ? ` in ${path.join(" / ")}` : ""}` : node.title;
}

function controlNode(path: readonly string[], id: string, title: string, kind: PipelineViewNode["kind"], row: BlueprintJournalNode | undefined): PipelineViewNode {
  return {
    ...baseNode(blueprintNodeKey(path, id), path.length ? `${title} · ${path.join(" / ")}` : title, kind),
    callPath: path, journal: row, state: row?.state ?? "waiting", current: row?.state === "running",
    lastEventAtMs: row?.ended_at_ms ?? row?.started_at_ms ?? null, resultSummary: row?.result_summary ?? row?.reason ?? null,
  };
}

function outcomes(node: BlueprintIrNode): readonly string[] {
  if (node.outcomes?.length) return node.outcomes;
  if (node.uses === "ask") return [...(node.buckets ?? []), "other"];
  if (node.uses === "http") return ["up", "down", "failed"];
  if (node.uses === "github") {
    const resource = node.with && typeof node.with === "object" ? (node.with as Record<string, { lit?: unknown }>).resource?.lit : undefined;
    return resource === "runners" ? ["online", "offline", "failed"] : resource === "pr_checks" ? ["done", "failed"] : resource === "run" || resource === "jobs" ? ["done", "failing", "failed"] : ["done", "failing", "failed", "online", "offline"];
  }
  const bucket = node.uses === "filter" && node.with && typeof node.with === "object" ? (node.with as Record<string, { lit?: unknown }>).bucket?.lit : undefined;
  return ["done", "failed", ...(typeof bucket === "string" ? [bucket] : [])];
}

export function blueprintTriggerSummary(trigger: BlueprintValue): string {
  if (!trigger || typeof trigger !== "object" || Array.isArray(trigger)) return "Trigger details unavailable";
  const fields = trigger as Readonly<Record<string, BlueprintValue>>;
  const literal = (value: BlueprintValue | undefined) => {
    if (value && typeof value === "object" && !Array.isArray(value)) return (value as Readonly<Record<string, BlueprintValue>>).lit;
    return value;
  };
  const kind = literal(fields.kind);
  if (fields.type === "schedule") {
    const every = literal(fields.every);
    return typeof every === "string" ? `Every ${every}` : "On schedule";
  }
  if (fields.type === "input") return typeof kind === "string" ? `Input: ${kind}` : "On input";
  if (fields.type === "event") return typeof kind === "string" ? `Event: ${kind}` : "On event";
  return "Trigger details unavailable";
}

export function* buildBlueprintGraph(pipeline: PipelineView, expanded: BlueprintExpansion): Generator<void, BlueprintGraphModel> {
  if (pipeline.historical_graph_available === false) return { nodes: [], edges: [] };
  if (!pipeline.call_graph) return { nodes: pipeline.nodes, edges: pipeline.edges };
  const nodes: PipelineViewNode[] = [];
  const edges: PipelineViewEdge[] = [];
  const rows = pipeline.nested_run && pipeline.nested_run.id === pipeline.runId
    && (pipeline.graph_definition_version == null || pipeline.nested_run.definition_version === pipeline.graph_definition_version)
    ? blueprintJournalNodes(pipeline).filter((row) => (row.run_id == null || row.run_id === pipeline.runId)
      && (pipeline.graph_definition_version == null || row.definition_version === pipeline.graph_definition_version)) : [];
  const journal = new Map<string, BlueprintJournalNode>();
  const byMarker = new Map<number, BlueprintJournalNode>();
  for (const row of rows) if (row.step_index != null && !byMarker.has(row.step_index)) byMarker.set(row.step_index, row);
  function scope(row: BlueprintJournalNode, seen = new Set<number>()): { document: BlueprintDocument; calls: readonly BlueprintExpandedCall[]; path: readonly string[] } | null {
    if (row.parent_step_index == null) return { document: pipeline.call_graph!, calls: pipeline.expanded_calls ?? [], path: [] };
    if (seen.has(row.parent_step_index)) return null;
    seen.add(row.parent_step_index);
    const parent = byMarker.get(row.parent_step_index);
    if (!parent) return null;
    const parentScope = scope(parent, seen);
    const call = parentScope?.calls.find((call) => call.node === parent.step_name);
    return call && parentScope ? { document: call.graph, calls: call.children, path: [...parentScope.path, parent.step_name] } : null;
  }
  for (const row of rows) {
    const context = scope(row);
    const control = CONTROL_ROWS[row.step_name];
    if (control && context) {
      const key = blueprintNodeKey(context.path, control);
      if (!journal.has(key)) journal.set(key, row);
      yield;
      continue;
    }
    const definition =context && [...context.document.steps, ...(context.document.other ?? [])].find((node) => node.id === row.step_name);
    const rootIndexMatches = row.step_index != null && [...pipeline.call_graph.steps, ...(pipeline.call_graph.other ?? [])][row.step_index]?.id === row.step_name;
    if (definition && context && (row.parent_step_index != null || row.definition_version != null || rootIndexMatches)) {
      const key = blueprintNodeKey(context.path, row.step_name);
      if (!journal.has(key)) journal.set(key, row);
    }
    yield;
  }
  function* graph(document: BlueprintDocument, calls: readonly BlueprintExpandedCall[], path: readonly string[]): Generator<void> {
    const steps = [...document.steps, ...(document.other ?? [])];
    const resultKey = blueprintNodeKey(path, "$result");
    if (document.trigger) {
      const key = blueprintNodeKey(path, "$trigger");
      nodes.push({ ...controlNode(path, "$trigger", "Trigger", "trigger", journal.get(key)), summary: blueprintTriggerSummary(document.trigger) });
      if (steps[0]) edges.push({ from: key, to: blueprintNodeKey(path, steps[0].id), label: null });
    }
    for (const [index, definition] of steps.entries()) {
      const key = blueprintNodeKey(path, definition.id);
      const row = journal.get(key);
      nodes.push({
        ...baseNode(key, definition.name ?? definition.id, "step"),
        capability: definition.call ? "call" : definition.code ? "code" : definition.run ? "command" : definition.uses,
        definition, callPath: path, journal: row,
        state: row?.state ?? "waiting", current: row?.state === "running",
        summary: definition.does ?? definition.id,
        lastEventAtMs: row?.ended_at_ms ?? row?.started_at_ms ?? null,
        resultSummary: row?.result_summary ?? row?.reason ?? null,
        outcome: row?.outcome ?? null,
      });
      const routes = new Set([...outcomes(definition), ...Object.keys(definition.routes ?? {})]);
      if (document.kind === "watch" && index === 0) routes.clear();
      if (document.kind === "watch" && index === 0 && steps[index + 1]) edges.push({ from: key, to: blueprintNodeKey(path, steps[index + 1].id), label: null });
      for (const outcome of routes) {
        const route = definition.routes?.[outcome] ?? (outcome === "failed" || document.kind === "watch" ? "@stop" : "@next");
        const target = route === "@next" ? steps[index + 1]?.id ?? "$result" : route === "@stop" ? "$stop" : route;
        edges.push({ from: key, to: blueprintNodeKey(path, target), label: outcome });
      }
      for (const [port, binding] of Object.entries(definition.bind ?? {})) {
        const split = binding.indexOf(".");
        if (split > 0) edges.push({ from: blueprintNodeKey(path, binding.slice(0, split)), to: key, label: `${binding.slice(split + 1)} → ${port}` });
      }
      yield;
      const call = calls.find((call) => call.node === definition.id);
      if (call && expanded.has(key)) {
        const childPath = [...path, definition.id];
        if (call.graph.steps[0]) edges.push({ from: key, to: blueprintNodeKey(childPath, call.graph.steps[0].id), label: `${call.pin.id}@${call.pin.hash}` });
        yield* graph(call.graph, call.children, childPath);
      }
    }
    if (edges.some((edge) => edge.to === blueprintNodeKey(path, "$stop"))) nodes.push(controlNode(path, "$stop", "Stop", "result", journal.get(blueprintNodeKey(path, "$stop"))));
    nodes.push({ ...controlNode(path, "$result", "Result", "result", journal.get(resultKey)), summary: document.result?.from ?? "", targetSummary: typeof document.result?.to === "string" ? document.result.to : null });
    yield;
  }
  yield* graph(pipeline.call_graph, pipeline.expanded_calls ?? [], []);
  return { nodes, edges };
}

export function expandedCall(projection: BlueprintProjection, path: readonly string[], id: string): BlueprintExpandedCall | undefined {
  let calls = projection.expanded_calls ?? [];
  for (const node of path) calls = calls.find((call) => call.node === node)?.children ?? [];
  return calls.find((call) => call.node === id);
}
