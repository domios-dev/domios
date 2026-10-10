import { nodeKind, bucketDefinitions, type ClassifierConfig } from "./nodeDecision";
import { automationKey, type AutomationGraph, type AutomationHandler, type AutomationHandlerResult, type AutomationNode } from "./types";
import { actionKind, actionTitle, type BlueprintNodeType, type TriggerKind, type SourceKind } from "./blueprintKinds";

/** Actions drawn per stage before the rest fold into one counted overflow node. */
export const MAX_ACTIONS_PER_STAGE = 24;

export type { BlueprintNodeType, TriggerKind, SourceKind } from "./blueprintKinds";
export { actionTitle } from "./blueprintKinds";
export type PinKind = "exec" | "data";

export interface BlueprintPin {
  id: string;
  kind: PinKind;
  side: "in" | "out";
  label: string;
  /** Second line under the label, such as a bucket's route definition. */
  detail?: string;
  bucket?: string;
  connected: boolean;
}

export interface BlueprintBody {
  kind: "code" | "prompt" | "text";
  text: string;
  language?: string;
}

export interface BlueprintNode {
  key: string;
  type: BlueprintNodeType;
  stageKey: string;
  stage: AutomationNode;
  title: string;
  subtitle: string;
  body: BlueprintBody | null;
  inputs: BlueprintPin[];
  outputs: BlueprintPin[];
  trigger?: TriggerKind;
  source?: SourceKind;
  action?: { bucket: string; handler: AutomationHandler | null; result: AutomationHandlerResult | null; target: AutomationNode | null; overflow: number };
}

export interface BlueprintWire {
  id: string;
  kind: PinKind;
  from: { node: string; pin: string };
  to: { node: string; pin: string };
  stageKey: string;
  bucket?: string;
  handlerId?: string;
  /** Closes a loop back to an earlier stage, so layout routes it under the graph. */
  back: boolean;
}

export interface BlueprintStage {
  key: string;
  node: AutomationNode;
  title: string;
  purpose: string | null;
  depth: number;
  nodes: string[];
}

export interface BlueprintModel {
  stages: BlueprintStage[];
  nodes: BlueprintNode[];
  wires: BlueprintWire[];
}

export const nodeKeys = (stageKey: string) => ({
  trigger: `${stageKey}|trigger`,
  source: `${stageKey}|source`,
  decision: `${stageKey}|decision`,
  action: (bucket: string, handler: string | null) => `${stageKey}|action|${JSON.stringify([bucket, handler])}`,
  overflow: `${stageKey}|overflow`,
});

export function wireId(from: string, pin: string, to: string) {
  return `${from}#${pin}->${to}`;
}

function every(ms: number) {
  if (ms % 3_600_000 === 0) return `${ms / 3_600_000} h`;
  if (ms % 60_000 === 0) return `${ms / 60_000} min`;
  return `${Math.round(ms / 1000)} s`;
}

function nameOf(graph: AutomationGraph, handler: AutomationHandler | null): AutomationNode | null {
  const target = handler?.target;
  return target ? graph.nodes.find((node) => automationKey(node.identity) === automationKey(target)) ?? null : null;
}

export function sourceKind(stage: AutomationNode): SourceKind {
  const type = stage.source?.type;
  if (type === "terminal" || type === "observe" || type === "input") return type;
  if (stage.detail?.observe_target) return "observe";
  return "probe";
}

function observedName(graph: AutomationGraph, stage: AutomationNode) {
  const target = stage.detail?.observe_target;
  const node = target ? graph.nodes.find((entry) => entry.identity.id === target.id) : null;
  return node?.name ?? stage.source?.display_name ?? (stage.source?.kind === "cron_run" ? "a cron run" : "a Team tab");
}

function sourceNode(graph: AutomationGraph, stage: AutomationNode) {
  const kind = sourceKind(stage);
  const source = stage.source;
  const probe = stage.detail?.definition?.probe ?? null;
  if (kind === "probe") return { title: "Run probe", subtitle: "Shell command", body: probe ? { kind: "code" as const, text: probe, language: stage.detail?.definition?.command_language } : null, output: "output" };
  if (kind === "terminal") {
    const rule = source?.pattern ? `When output matches /${source.pattern}/` : source?.idle_ms ? `When output settles for ${source.idle_ms} ms` : "On new output";
    return { title: `Terminal: ${observedName(graph, stage)}`, subtitle: "Watches a Team terminal", body: { kind: "text" as const, text: rule }, output: "new output" };
  }
  if (kind === "observe") return { title: `Observe: ${observedName(graph, stage)}`, subtitle: source?.kind === "cron_run" ? "Watches a cron run" : "Watches a Team tab", body: null, output: "report" };
  const media = source?.kind ?? "text";
  const reference = source?.ref_type === "text" ? "inline text" : source?.display_name || source?.ref_type || "input";
  return { title: `Input: ${media}`, subtitle: reference, body: source?.convert_to ? { kind: "text" as const, text: `Converted to ${source.convert_to} before the decision` } : null, output: source?.convert_to ?? media };
}

function triggerNode(stage: AutomationNode, activators: readonly AutomationNode[]): { kind: TriggerKind; title: string; subtitle: string } {
  const cadence = stage.cadence_ms ? `every ${every(stage.cadence_ms)}` : stage.schedule ?? null;
  if (activators.length) return { kind: "activated", title: `After ${activators.map((node) => node.name).join(", ")}`, subtitle: cadence ? `Then ${cadence}` : "Runs when activated" };
  const kind = sourceKind(stage);
  if (kind === "terminal" || kind === "observe") return { kind: "event", title: "On new activity", subtitle: kind === "terminal" ? "Terminal output event" : "Observed report event" };
  if (kind === "input") return { kind: "manual", title: "On input", subtitle: "codeterm blueprint input" };
  return { kind: "schedule", title: cadence ? `Every ${cadence.replace(/^every /, "")}` : "On schedule", subtitle: stage.expires_at ? "Until it expires" : "Schedule" };
}

/** Visits stages from the scenario roots; a stage activated by a deeper or same-depth stage closes a loop. */
function stageDepths(stages: readonly AutomationNode[], roots: readonly string[], next: (key: string) => string[]) {
  const depth = new Map<string, number>();
  const queue = [...roots, ...stages.map((stage) => automationKey(stage.identity))];
  for (const key of queue) {
    if (depth.has(key)) continue;
    depth.set(key, 0);
    const frontier = [key];
    while (frontier.length) {
      const current = frontier.shift()!;
      for (const target of next(current)) {
        if (depth.has(target)) continue;
        depth.set(target, depth.get(current)! + 1);
        frontier.push(target);
      }
    }
  }
  return depth;
}

/** Explodes every stored stage into Trigger → Source → Agent|Logic → Action nodes with typed pins and wires. */
export function blueprintModel(graph: AutomationGraph, classifier: ClassifierConfig = graph.configured_fallback): BlueprintModel {
  const pipelineIds = new Set(graph.blueprints?.map((blueprint) => blueprint.id));
  const stages = graph.nodes.filter((node) => node.identity.kind === "blueprint" && !pipelineIds.has(node.identity.id) && node.availability !== "missing");
  const stageKeys = new Set(stages.map((stage) => automationKey(stage.identity)));
  const chained = (stage: AutomationNode) => stage.outcomes.flatMap((outcome) => outcome.handlers.filter((handler) => actionKind(handler.type)?.chain && handler.target && stageKeys.has(automationKey(handler.target))).map((handler) => ({ bucket: outcome.bucket, handler, target: automationKey(handler.target!) })));
  const outgoing = new Map(stages.map((stage) => [automationKey(stage.identity), chained(stage)]));
  const roots = graph.scenarios.map((scenario) => automationKey(scenario.root)).filter((key) => stageKeys.has(key));
  const depth = stageDepths(stages, roots, (key) => (outgoing.get(key) ?? []).map((link) => link.target));
  const loops = (from: string, to: string) => depth.get(to)! <= depth.get(from)!;
  const activators = new Map<string, AutomationNode[]>();
  const looped = new Set<string>();
  for (const stage of stages) for (const link of outgoing.get(automationKey(stage.identity)) ?? []) {
    if (loops(automationKey(stage.identity), link.target)) { looped.add(link.target); continue; }
    const list = activators.get(link.target) ?? [];
    if (!list.includes(stage)) list.push(stage);
    activators.set(link.target, list);
  }

  const nodes: BlueprintNode[] = [];
  const wires: BlueprintWire[] = [];
  const result: BlueprintStage[] = [];
  const connect = (kind: PinKind, from: string, fromPin: string, to: string, toPin: string, stageKey: string, extra: Partial<BlueprintWire> = {}) =>
    wires.push({ id: wireId(from, fromPin, to), kind, from: { node: from, pin: fromPin }, to: { node: to, pin: toPin }, stageKey, back: false, ...extra });

  const ordered = [...stages].sort((a, b) => depth.get(automationKey(a.identity))! - depth.get(automationKey(b.identity))!);
  for (const stage of ordered) {
    const stageKey = automationKey(stage.identity);
    const keys = nodeKeys(stageKey);
    const ownActivators = activators.get(stageKey) ?? [];
    const trigger = triggerNode(stage, ownActivators);
    nodes.push({
      key: keys.trigger, type: "trigger", stageKey, stage, trigger: trigger.kind, title: trigger.title, subtitle: trigger.subtitle, body: null,
      inputs: trigger.kind === "activated" || looped.has(stageKey) ? [{ id: "activate", kind: "exec", side: "in", label: "", connected: true }] : [],
      outputs: [{ id: "then", kind: "exec", side: "out", label: "", connected: true }],
    });
    const source = sourceNode(graph, stage);
    nodes.push({
      key: keys.source, type: "source", stageKey, stage, source: sourceKind(stage), title: source.title, subtitle: source.subtitle, body: source.body,
      inputs: [{ id: "exec", kind: "exec", side: "in", label: "", connected: true }],
      outputs: [{ id: "then", kind: "exec", side: "out", label: "", connected: true }, { id: "output", kind: "data", side: "out", label: source.output, connected: true }],
    });

    const kind = nodeKind(stage, classifier);
    const definition = stage.detail?.definition;
    const routes = bucketDefinitions(stage);
    const buckets = [...routes, ...stage.outcomes.filter((outcome) => !routes.some((route) => route.name === outcome.bucket)).map((outcome) => ({ name: outcome.bucket, description: "" }))];
    const declared = (bucket: string) => stage.outcomes.find((outcome) => outcome.bucket === bucket)?.handlers ?? [];
    const patterns = definition?.buckets?.filter((bucket) => bucket.pattern).map((bucket) => `${bucket.name}: /${bucket.pattern}/`) ?? [];
    nodes.push({
      key: keys.decision, type: kind.ai ? "agent" : "logic", stageKey, stage,
      title: kind.ai ? kind.label : patterns.length ? "Pattern match" : "Exit-code rules",
      subtitle: kind.ai ? (patterns.length ? "Decides after pattern rules" : "Agent decision") : "Deterministic rules",
      body: kind.ai ? (definition?.question ? { kind: "prompt", text: definition.question } : null) : patterns.length ? { kind: "text", text: patterns.join("\n") } : null,
      inputs: [{ id: "exec", kind: "exec", side: "in", label: "", connected: true }, { id: "receives", kind: "data", side: "in", label: `receives ${source.output}`, connected: true }],
      outputs: buckets.map((bucket) => ({ id: `bucket:${bucket.name}`, kind: "exec", side: "out", label: bucket.name, detail: bucket.description || undefined, bucket: bucket.name, connected: declared(bucket.name).length > 0 })),
    });
    connect("exec", keys.trigger, "then", keys.source, "exec", stageKey);
    connect("exec", keys.source, "then", keys.decision, "exec", stageKey);
    connect("data", keys.source, "output", keys.decision, "receives", stageKey);

    const stageNodes = [keys.trigger, keys.source, keys.decision];
    let drawn = 0;
    let overflow = 0;
    for (const bucket of buckets) for (const handler of declared(bucket.name)) {
      if (drawn >= MAX_ACTIONS_PER_STAGE) { overflow++; continue; }
      drawn++;
      const key = keys.action(bucket.name, handler.id);
      const target = nameOf(graph, handler);
      const accepted = stage.last_result?.bucket === bucket.name ? stage.last_result.handlers.find((entry) => entry.handler_id === handler.id) ?? null : null;
      const label = typeof handler.label === "string" && handler.label.trim() ? handler.label : null;
      const command = actionKind(handler.type)?.code ? definition?.commands?.find((entry) => entry.bucket === bucket.name && entry.handler_id === handler.id) : undefined;
      const chain = actionKind(handler.type)?.chain && handler.target && stageKeys.has(automationKey(handler.target));
      nodes.push({
        key, type: "action", stageKey, stage, title: actionTitle(handler.type, target?.name ?? null, label),
        subtitle: target?.executor?.model ? `${target.executor.provider_id ?? "agent"} · ${target.executor.model}` : `on ${bucket.name}`,
        body: typeof command?.command === "string" ? { kind: "code", text: command.command, language: definition?.command_language } : null,
        inputs: [{ id: "exec", kind: "exec", side: "in", label: "", connected: true }, { id: "payload", kind: "data", side: "in", label: "payload", connected: false }],
        outputs: chain ? [{ id: "then", kind: "exec", side: "out", label: "", connected: true }] : [],
        action: { bucket: bucket.name, handler, result: accepted ?? handler.last_result ?? null, target, overflow: 0 },
      });
      stageNodes.push(key);
      connect("exec", keys.decision, `bucket:${bucket.name}`, key, "exec", stageKey, { bucket: bucket.name, handlerId: handler.id });
      if (chain) {
        const targetKey = automationKey(handler.target!);
        connect("exec", key, "then", nodeKeys(targetKey).trigger, "activate", stageKey, { bucket: bucket.name, handlerId: handler.id, back: loops(stageKey, targetKey) });
      }
    }
    if (overflow) {
      nodes.push({ key: keys.overflow, type: "action", stageKey, stage, title: `+${overflow} more actions`, subtitle: "Open the decision for every route", body: null, inputs: [], outputs: [], action: { bucket: "", handler: null, result: null, target: null, overflow } });
      stageNodes.push(keys.overflow);
    }
    result.push({ key: stageKey, node: stage, title: stage.name, purpose: stage.purpose, depth: depth.get(stageKey)!, nodes: stageNodes });
  }
  return { stages: result, nodes, wires };
}
