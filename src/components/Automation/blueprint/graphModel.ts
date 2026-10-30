import { CircleStop } from "lucide-react";
import type { BlueprintDocument, BlueprintExpandedCall, BlueprintIrNode, BlueprintJournalNode, BlueprintPort, BlueprintProjection, BlueprintValue } from "../../../lib/blueprintApi";
import type { PipelineView } from "../pipelineModel";
import { blueprintNodeKey, type BlueprintExpansion } from "../blueprintExpansion";
import { blueprintJournalNodes } from "../blueprintPipelineOutcome";
import { blueprintNodeKinds, irNodeKind, triggerIcon, type BlueprintNodeType, type NodeIcon } from "./kinds";
import { nodeTrigger, type BlueprintNodeTrigger } from "./triggerModel";

export type PinKind = "exec" | "data";

export interface BlueprintPin {
  id: string;
  kind: PinKind;
  side: "in" | "out";
  label: string;
  detail?: string;
  /** The port's schema type when the definition declares it, e.g. from `in.properties.<port>`. */
  type?: string;
  connected: boolean;
}

export interface BlueprintBody {
  kind: "code" | "prompt" | "text";
  text: string;
  language?: string;
}

export interface NodeEvidence {
  state: string;
  outcome: string | null;
  summary: string | null;
  at: number | null;
  journal?: BlueprintJournalNode;
}

/** `definition` shows no run; `unavailable` is a selected run that kept no per-node evidence. */
export type EvidenceMode = "definition" | "run" | "unavailable";

export interface BlueprintGraphNode {
  key: string;
  id: string;
  type: BlueprintNodeType;
  icon: NodeIcon;
  kindLabel: string;
  title: string;
  body: BlueprintBody | null;
  inputs: BlueprintPin[];
  outputs: BlueprintPin[];
  callPath: readonly string[];
  scope: string;
  definition?: BlueprintIrNode;
  control?: "trigger" | "result" | "stop";
  trigger?: BlueprintNodeTrigger;
  routes: { outcome: string; target: string }[];
  evidence: NodeEvidence | null;
  /** A call node's sub-graph: the callee's name and whether its body is here to open. */
  callee?: { name: string; openable: boolean };
}

export interface BlueprintGraphWire {
  id: string;
  kind: PinKind;
  from: { node: string; pin: string };
  to: { node: string; pin: string };
  outcome: string | null;
  back: boolean;
}

export interface BlueprintGraphScope {
  key: string;
  path: readonly string[];
  title: string;
  detail: string | null;
  call: string | null;
}

export interface BlueprintGraphModel {
  nodes: BlueprintGraphNode[];
  wires: BlueprintGraphWire[];
  scopes: BlueprintGraphScope[];
  mode: EvidenceMode;
}

const CONTROL_ROWS: Readonly<Record<string, string>> = { $run: "$trigger", $result: "$result", $stop: "$stop" };
const ACTIVE_RUN_STATES = new Set(["running", "delivering", "active"]);

export function nodeLabel(node: Pick<BlueprintGraphNode, "id" | "title" | "callPath" | "control">): string {
  const path = node.callPath.length ? ` in ${node.callPath.join(" / ")}` : "";
  return `${node.control ? node.title : node.id}${path}`;
}

export function scopeKey(path: readonly string[]): string {
  return JSON.stringify(path);
}

export function wireId(from: string, pin: string, to: string, target: string): string {
  return `${from}#${pin}->${to}#${target}`;
}

function literal(value: BlueprintValue | undefined): BlueprintValue | undefined {
  if (value && typeof value === "object" && !Array.isArray(value) && "lit" in value) return (value as Readonly<Record<string, BlueprintValue>>).lit;
  return value;
}

function text(value: BlueprintValue | undefined): string | null {
  const plain = literal(value);
  if (plain === undefined || plain === null) return null;
  return typeof plain === "string" ? plain : typeof plain === "number" || typeof plain === "boolean" ? String(plain) : JSON.stringify(plain);
}

function field(node: BlueprintIrNode, name: string): string | null {
  const values = node.with && typeof node.with === "object" ? node.with as Readonly<Record<string, BlueprintValue>> : {};
  return text(values[name]);
}

export function portType(port: BlueprintPort | undefined): string {
  if (!port) return "unspecified";
  const record = port as Readonly<Record<string, unknown>>;
  const type = typeof record.type === "string" ? record.type : typeof record.$ref === "string" ? record.$ref : "value";
  return typeof record["x-media"] === "string" ? `${type} · ${record["x-media"]}` : type;
}

export function blueprintTriggerSummary(trigger: BlueprintValue): string {
  if (!trigger || typeof trigger !== "object" || Array.isArray(trigger)) return "Trigger details unavailable";
  const fields = trigger as Readonly<Record<string, BlueprintValue>>;
  const kind = literal(fields.kind);
  if (fields.type === "schedule") {
    const every = literal(fields.every);
    return typeof every === "string" ? `Every ${every}` : "On schedule";
  }
  if (fields.type === "input") return typeof kind === "string" ? `Input: ${kind}` : "On input";
  if (fields.type === "event") return typeof kind === "string" ? `Event: ${kind}` : "On event";
  return "Trigger details unavailable";
}

function outcomes(node: BlueprintIrNode): readonly string[] {
  if (node.outcomes?.length) return node.outcomes;
  if (node.uses === "ask") return [...(node.buckets ?? []), "other"];
  if (node.uses === "http") return ["up", "down", "failed"];
  if (node.uses === "github") {
    const resource = field(node, "resource");
    return resource === "runners" ? ["online", "offline", "failed"] : resource === "pr_checks" ? ["done", "failed"] : resource === "run" || resource === "jobs" ? ["done", "failing", "failed"] : ["done", "failing", "failed", "online", "offline"];
  }
  const bucket = node.uses === "filter" ? field(node, "bucket") : null;
  return ["done", "failed", ...(bucket ? [bucket] : [])];
}

function bodyOf(node: BlueprintIrNode): BlueprintBody | null {
  if (node.code) return { kind: "code", text: node.code.src ?? `file: ${node.code.file ?? "unavailable"}`, language: node.code.lang };
  if (node.run !== undefined) return { kind: "code", text: text(node.run) ?? "", language: "shell" };
  if (node.uses === "probe") { const command = field(node, "command"); return command ? { kind: "code", text: command, language: "shell" } : null; }
  if (node.uses === "ask") { const question = field(node, "question") ?? field(node, "prompt"); return question ? { kind: "prompt", text: question } : null; }
  if (node.uses === "http") { const url = field(node, "url"); return url ? { kind: "text", text: url } : null; }
  if (node.uses === "filter") { const pattern = field(node, "pattern"); return pattern ? { kind: "text", text: `Matches /${pattern}/` } : null; }
  if (node.uses === "convert") { const from = field(node, "from"), to = field(node, "to"); return from && to ? { kind: "text", text: `${from} → ${to}` } : null; }
  return node.does ? { kind: "text", text: node.does } : null;
}

function bindings(node: BlueprintIrNode): [string, string][] {
  const bound: [string, string][] = Object.entries(node.bind ?? {});
  const values = { ...(node.with && typeof node.with === "object" ? node.with as Readonly<Record<string, BlueprintValue>> : {}), ...(node.call?.with ?? {}) };
  for (const [port, value] of Object.entries(values)) {
    const bind = value && typeof value === "object" && !Array.isArray(value) ? (value as Readonly<Record<string, BlueprintValue>>).bind : undefined;
    if (typeof bind === "string" && !bound.some(([name]) => name === port)) bound.push([port, bind]);
  }
  return bound;
}

export function splitBinding(binding: string): { node: string; port: string } | null {
  const [node, port, ...fields] = binding.split(".");
  if (!node || node.length > 64 || !/^[a-zA-Z0-9_-]+$/.test(node) || node === "next" || node === "stop" || port !== "out" || fields.some((field) => !field)) return null;
  return { node, port };
}

function namedInputType(input: BlueprintPort | undefined, port: string): string | undefined {
  const properties = input && typeof input === "object" ? (input as Readonly<Record<string, unknown>>).properties : undefined;
  const schema = properties && typeof properties === "object" ? (properties as Readonly<Record<string, BlueprintPort>>)[port] : undefined;
  return schema ? portType(schema) : undefined;
}

/** A callee is named by its own document; raw memory ids stay in the details. */
export function calleeName(call: NonNullable<BlueprintIrNode["call"]>, expanded?: BlueprintExpandedCall): string {
  if (expanded?.graph.name) return expanded.graph.name;
  const ref = call.pin?.id ?? call.ref;
  return ref.startsWith("mem:skill_blueprint_solution_") ? "Saved solution" : ref.replace(/^mem:/, "");
}

function calleeOf(call: NonNullable<BlueprintIrNode["call"]>, expanded?: BlueprintExpandedCall): { name: string; openable: boolean } {
  return { name: calleeName(call, expanded), openable: Boolean(expanded) };
}

function journalEvidence(pipeline: PipelineView): Map<string, NodeEvidence> {
  const evidence = new Map<string, NodeEvidence>();
  const root = pipeline.call_graph;
  if (!root || !pipeline.nested_run || pipeline.nested_run.id !== pipeline.runId
    || (pipeline.graph_definition_version != null && pipeline.nested_run.definition_version !== pipeline.graph_definition_version)) return evidence;
  const rows = blueprintJournalNodes(pipeline).filter((row) => (row.run_id == null || row.run_id === pipeline.runId)
    && (pipeline.graph_definition_version == null || row.definition_version === pipeline.graph_definition_version));
  const byMarker = new Map<number, BlueprintJournalNode>();
  for (const row of rows) if (row.step_index != null && !byMarker.has(row.step_index)) byMarker.set(row.step_index, row);
  const scope = (row: BlueprintJournalNode, seen = new Set<number>()): { document: BlueprintDocument; calls: readonly BlueprintExpandedCall[]; path: readonly string[] } | null => {
    if (row.parent_step_index == null) return { document: root, calls: pipeline.expanded_calls ?? [], path: [] };
    if (seen.has(row.parent_step_index)) return null;
    seen.add(row.parent_step_index);
    const parent = byMarker.get(row.parent_step_index);
    const parentScope = parent ? scope(parent, seen) : null;
    const call = parentScope?.calls.find((entry) => entry.node === parent!.step_name);
    return call && parentScope ? { document: call.graph, calls: call.children, path: [...parentScope.path, parent!.step_name] } : null;
  };
  const rootSteps = [...root.steps, ...(root.other ?? [])];
  for (const row of rows) {
    const context = scope(row);
    if (!context) continue;
    const control = CONTROL_ROWS[row.step_name];
    const definition = [...context.document.steps, ...(context.document.other ?? [])].some((node) => node.id === row.step_name);
    const rootIndexMatches = row.step_index != null && rootSteps[row.step_index]?.id === row.step_name;
    if (!control && !(definition && (row.parent_step_index != null || row.definition_version != null || rootIndexMatches))) continue;
    const key = blueprintNodeKey(context.path, control ?? row.step_name);
    if (evidence.has(key)) continue;
    if (row.step_name === "$run") evidence.set(key, { state: row.state === "running" ? "running" : "done", outcome: null, summary: "Run started", at: row.started_at_ms ?? null, journal: row });
    else evidence.set(key, { state: row.state, outcome: row.outcome ?? null, summary: [...new Set([row.reason, row.result_summary].filter(Boolean))].join(" · ") || null, at: row.ended_at_ms ?? row.started_at_ms ?? null, journal: row });
  }
  return evidence;
}

function runEvidence(pipeline: PipelineView): { mode: EvidenceMode; evidence: Map<string, NodeEvidence> } {
  if (pipeline.retained) {
    const evidence = new Map(pipeline.retained.steps.map((step) => [blueprintNodeKey([], step.step), { state: step.state, outcome: step.outcome, summary: step.summary, at: step.at }]));
    return { mode: evidence.size ? "run" : "unavailable", evidence };
  }
  if (pipeline.runId == null) return { mode: "definition", evidence: new Map() };
  const evidence = journalEvidence(pipeline);
  return { mode: evidence.size ? "run" : "unavailable", evidence };
}

function unreached(runState: string): NodeEvidence {
  return { state: ACTIVE_RUN_STATES.has(runState) ? "pending" : "not reached", outcome: null, summary: null, at: null };
}

const pin = (id: string, kind: PinKind, side: "in" | "out", label: string, detail?: string): BlueprintPin => ({ id, kind, side, label, detail, connected: false });

/** A Blueprint known only by its step snapshot still reads as a typed chain; its live step states are the evidence. */
function snapshotGraph(pipeline: PipelineView): BlueprintGraphModel {
  const scope = scopeKey([]);
  const evidenceOf = (state: string, summary: string | null, at: number | null, outcome: string | null): NodeEvidence | null => state === "waiting" ? null : { state, outcome, summary, at };
  const nodes: BlueprintGraphNode[] = pipeline.nodes.map((entry, index) => {
    const kind = entry.kind === "trigger" ? { type: "trigger" as const, icon: triggerIcon(undefined), label: "Trigger" }
      : entry.kind === "result" ? { type: "result" as const, icon: blueprintNodeKinds.result.icon, label: "Result" }
        : irNodeKind({ id: entry.key, uses: entry.capability }, false);
    const outgoing = pipeline.edges.filter((edge) => edge.from === entry.key);
    return {
      key: entry.key, id: entry.kind === "step" ? entry.title : entry.key, type: kind.type, icon: kind.icon, kindLabel: kind.label, title: entry.title,
      body: entry.summary ? { kind: "text", text: entry.kind === "result" ? entry.targetSummary ?? entry.summary : entry.summary } : null,
      inputs: index === 0 ? [] : [pin("in", "exec", "in", "")],
      outputs: outgoing.map((edge) => pin(`route:${edge.label ?? "next"}`, "exec", "out", edge.label ?? "")),
      callPath: [], scope, control: entry.kind === "step" ? undefined : entry.kind, routes: outgoing.map((edge) => ({ outcome: edge.label ?? "next", target: edge.to })),
      evidence: entry.kind === "trigger" ? null : evidenceOf(entry.sourceState ?? entry.state, entry.resultSummary ?? entry.reason ?? null, entry.lastEventAtMs, entry.outcome),
    };
  });
  const wires = pipeline.edges.map((edge): BlueprintGraphWire => {
    const fromPin = `route:${edge.label ?? "next"}`;
    return { id: wireId(edge.from, fromPin, edge.to, "in"), kind: "exec", from: { node: edge.from, pin: fromPin }, to: { node: edge.to, pin: "in" }, outcome: edge.label, back: false };
  });
  for (const node of nodes) for (const entry of [...node.inputs, ...node.outputs]) entry.connected = wires.some((wire) => (wire.from.node === node.key && wire.from.pin === entry.id) || (wire.to.node === node.key && wire.to.pin === entry.id));
  return { nodes, wires, scopes: [{ key: scope, path: [], title: pipeline.name, detail: null, call: null }], mode: nodes.some((node) => node.evidence) ? "run" : "definition" };
}

export function* buildBlueprintGraph(pipeline: PipelineView, expanded: BlueprintExpansion): Generator<void, BlueprintGraphModel> {
  if (!pipeline.call_graph && pipeline.historical_graph_available !== false) return snapshotGraph(pipeline);
  const { mode, evidence } = runEvidence(pipeline);
  if (pipeline.historical_graph_available === false || !pipeline.call_graph) return { nodes: [], wires: [], scopes: [], mode };
  const nodes: BlueprintGraphNode[] = [];
  const wires: BlueprintGraphWire[] = [];
  const scopes: BlueprintGraphScope[] = [];
  const reached = evidence.size > 0;

  function evidenceFor(key: string, control?: BlueprintGraphNode["control"]): NodeEvidence | null {
    if (mode !== "run") return null;
    const found = evidence.get(key);
    if (found) return found;
    if (control === "trigger" && reached) return { state: "done", outcome: null, summary: null, at: null };
    return unreached(pipeline.state);
  }

  function* graph(document: BlueprintDocument, calls: readonly BlueprintExpandedCall[], path: readonly string[], call: BlueprintExpandedCall | null): Generator<void> {
    const scope = scopeKey(path);
    scopes.push({ key: scope, path, title: call ? call.graph.name || call.pin.id : document.name, detail: call ? `@${call.pin.hash.slice(0, 12)}` : null, call: call ? blueprintNodeKey(path.slice(0, -1), call.node) : null });
    const steps = [...document.steps, ...(document.other ?? [])];
    const local = new Map<string, BlueprintGraphNode>();
    const order: string[] = [];
    const add = (node: BlueprintGraphNode) => { nodes.push(node); local.set(node.id, node); order.push(node.key); };
    const control = (id: string, title: string, type: BlueprintNodeType, kind: BlueprintGraphNode["control"], icon: NodeIcon): BlueprintGraphNode => {
      const key = blueprintNodeKey(path, id);
      return { key, id, type, icon, kindLabel: blueprintNodeKinds[type].label, title, body: null, inputs: [], outputs: [], callPath: path, scope, control: kind, routes: [], evidence: evidenceFor(key, kind) };
    };
    const decisionIsDeterministic = (node: BlueprintIrNode) => node.model_classification === false || Boolean(node.matches);

    if (document.trigger) {
      const trigger = control("$trigger", "Trigger", "trigger", "trigger", triggerIcon((document.trigger as Readonly<Record<string, unknown>>).type));
      trigger.trigger = nodeTrigger(document.trigger, true, path.length ? null : pipeline.triggerRuntime ?? null);
      trigger.body = { kind: "text", text: blueprintTriggerSummary(document.trigger) };
      trigger.outputs.push(pin("next", "exec", "out", ""));
      const fields = document.trigger as Readonly<Record<string, BlueprintValue>>;
      if (fields.type === "input") trigger.outputs.push(pin("value", "data", "out", text(fields.kind) ?? "input"));
      add(trigger);
    }
    for (const [index, definition] of steps.entries()) {
      const kind = irNodeKind(definition, decisionIsDeterministic(definition));
      const key = blueprintNodeKey(path, definition.id);
      const node: BlueprintGraphNode = {
        key, id: definition.id, type: kind.type, icon: kind.icon, kindLabel: kind.label, title: definition.name ?? definition.id,
        body: bodyOf(definition), inputs: [pin("in", "exec", "in", "")], outputs: [], callPath: path, scope, definition, routes: [], evidence: evidenceFor(key),
        trigger: nodeTrigger(definition, document.kind === "watch" && index === 0, !path.length && document.kind === "watch" && index === 0 ? pipeline.triggerRuntime ?? null : null),
        callee: definition.call ? calleeOf(definition.call, calls.find((entry) => entry.node === definition.id)) : undefined,
      };
      const bound = bindings(definition);
      for (const [port, binding] of bound) node.inputs.push({ ...pin(`bind:${port}`, "data", "in", port, binding), type: namedInputType(definition.in, port) ?? (port === "in" && definition.in ? portType(definition.in) : undefined) });
      if (!bound.length && definition.in) node.inputs.push(pin("data:in", "data", "in", "in", portType(definition.in)));
      if (document.kind === "watch" && index === 0 && steps[1]) {
        node.outputs.push(pin("next", "exec", "out", ""));
        node.routes.push({ outcome: "next", target: steps[1].id });
      } else {
        for (const outcome of new Set([...outcomes(definition), ...Object.keys(definition.routes ?? {})])) {
          const route = definition.routes?.[outcome] ?? (outcome === "failed" || document.kind === "watch" ? "@stop" : "@next");
          const target = route === "@next" ? steps[index + 1]?.id ?? "$result" : route === "@stop" ? "$stop" : route;
          node.routes.push({ outcome, target });
          node.outputs.push(pin(`route:${outcome}`, "exec", "out", outcome));
        }
      }
      if (definition.out) node.outputs.push(pin("data:out", "data", "out", "out", portType(definition.out)));
      add(node);
      yield;
      const callee = calls.find((entry) => entry.node === definition.id);
      if (callee && expanded.has(key)) {
        node.outputs.push(pin("callee", "exec", "out", "callee", callee.pin.id));
        yield* graph(callee.graph, callee.children, [...path, definition.id], callee);
      }
    }
    const routesTo = (target: string) => [...local.values()].some((node) => node.routes.some((route) => route.target === target));
    if (routesTo("$stop")) {
      const stop = control("$stop", "Stop", "result", "stop", CircleStop);
      stop.inputs.push(pin("in", "exec", "in", ""));
      add(stop);
    }
    const result = control("$result", "Result", "result", "result", blueprintNodeKinds.result.icon);
    result.inputs.push(pin("in", "exec", "in", ""));
    const resultTo = text(document.result?.to);
    if (resultTo) result.body = { kind: "text", text: `Delivers to ${resultTo}` };
    if (document.result?.from) result.inputs.push(pin("bind:value", "data", "in", "value", document.result.from));
    add(result);

    const index = new Map(order.map((key, position) => [key, position]));
    const connect = (kind: PinKind, from: BlueprintGraphNode, fromPin: string, to: BlueprintGraphNode, toPin: string, outcome: string | null) => {
      wires.push({ id: wireId(from.key, fromPin, to.key, toPin), kind, from: { node: from.key, pin: fromPin }, to: { node: to.key, pin: toPin }, outcome, back: kind === "exec" && index.get(to.key)! <= index.get(from.key)! });
    };
    const first = steps[0] ? local.get(steps[0].id) : undefined;
    const trigger = local.get("$trigger");
    if (trigger && first) {
      connect("exec", trigger, "next", first, "in", null);
      if (trigger.outputs.some((entry) => entry.id === "value") && first.inputs.some((entry) => entry.id === "data:in")) connect("data", trigger, "value", first, "data:in", null);
    }
    for (const node of local.values()) {
      for (const route of node.routes) {
        const target = local.get(route.target);
        const fromPin = route.outcome === "next" && node.outputs.some((entry) => entry.id === "next") ? "next" : `route:${route.outcome}`;
        if (target) connect("exec", node, fromPin, target, "in", route.outcome === "next" ? null : route.outcome);
      }
      for (const input of node.inputs.filter((entry) => entry.id.startsWith("bind:"))) {
        const source = splitBinding(input.detail ?? "");
        if (!source) continue;
        const from = local.get(source.node);
        if (!from) continue;
        const id = source.port === "out" ? "data:out" : `data:${source.port}`;
        if (!from.outputs.some((entry) => entry.id === id)) from.outputs.push(pin(id, "data", "out", source.port));
        connect("data", from, id, node, input.id, null);
      }
    }
    const callee = call && nodes.find((node) => node.key === blueprintNodeKey(path.slice(0, -1), call.node));
    const entry = trigger ?? first;
    if (callee && entry) wires.push({ id: wireId(callee.key, "callee", entry.key, "in"), kind: "exec", from: { node: callee.key, pin: "callee" }, to: { node: entry.key, pin: "in" }, outcome: null, back: false });
    yield;
  }

  yield* graph(pipeline.call_graph, pipeline.expanded_calls ?? [], [], null);
  const pins = new Set(wires.flatMap((wire) => [`${wire.from.node}#${wire.from.pin}`, `${wire.to.node}#${wire.to.pin}`]));
  for (const node of nodes) for (const entry of [...node.inputs, ...node.outputs]) entry.connected = pins.has(`${node.key}#${entry.id}`);
  return { nodes, wires, scopes, mode };
}

/** The callee scope at `trail` as its own graph, with its nested callees; evidence comes along unchanged. */
export function focusScope(model: BlueprintGraphModel, trail: readonly string[]): BlueprintGraphModel {
  if (!trail.length) return model;
  const within = (path: readonly string[]) => path.length >= trail.length && trail.every((id, index) => path[index] === id);
  const nodes = model.nodes.filter((node) => within(node.callPath));
  const keys = new Set(nodes.map((node) => node.key));
  const focused = scopeKey(trail);
  const scopes = model.scopes.filter((scope) => within(scope.path)).map((scope) => scope.key === focused ? { ...scope, call: null } : scope);
  scopes.sort((a, b) => a.path.length - b.path.length);
  return { nodes, wires: model.wires.filter((wire) => keys.has(wire.from.node) && keys.has(wire.to.node)), scopes, mode: model.mode };
}

export function litWires(model: BlueprintGraphModel): Set<string> {
  const byKey = new Map(model.nodes.map((node) => [node.key, node]));
  const acted = (node: BlueprintGraphNode | undefined) => Boolean(node?.evidence && !["not reached", "pending", "skipped"].includes(node.evidence.state));
  return new Set(model.wires.filter((wire) => {
    const from = byKey.get(wire.from.node), to = byKey.get(wire.to.node);
    if (!acted(from) || !acted(to)) return false;
    return wire.outcome === null || !from!.evidence!.outcome || from!.evidence!.outcome === wire.outcome;
  }).map((wire) => wire.id));
}

export function expandedCall(projection: BlueprintProjection, path: readonly string[], id: string): BlueprintExpandedCall | undefined {
  let calls = projection.expanded_calls ?? [];
  for (const node of path) calls = calls.find((call) => call.node === node)?.children ?? [];
  return calls.find((call) => call.node === id);
}
