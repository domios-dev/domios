import { automationKey, type AutomationGraph, type AutomationScenario, type BlueprintOwner, type BlueprintResultState, type BlueprintSnapshot, type BlueprintStepState } from "./types";

export interface PipelineViewNode {
  key: string;
  kind: "trigger" | "step" | "result";
  title: string;
  state: string;
  current: boolean;
  summary: string;
  lastEventAtMs: number | null;
  resultSummary: string | null;
  outcome: string | null;
  targetSummary: string | null;
}

export interface PipelineViewEdge {
  from: string;
  to: string;
  label: string | null;
}

export interface PipelineView {
  id: string;
  name: string;
  owner: BlueprintOwner;
  state: string;
  nodes: readonly PipelineViewNode[];
  edges: readonly PipelineViewEdge[];
}

export interface PipelineOwnerGroup {
  key: string;
  kind: "tab" | "team" | "unbound";
  ownerId: string | null;
  blueprints: readonly PipelineView[];
}

function normalizedOwner(owner: BlueprintOwner | undefined): BlueprintOwner {
  return owner ?? { kind: "tab", tab_id: null, team_id: null };
}

function currentStepIndex(blueprint: BlueprintSnapshot): number | null {
  const steps = blueprint.steps ?? [];
  if (blueprint.current_step !== null && blueprint.current_step !== undefined) {
    return steps.some((step) => step.index === blueprint.current_step) ? blueprint.current_step : null;
  }
  return steps.find((step) => step.state === "running")?.index ?? null;
}

function targetSummary(blueprint: BlueprintSnapshot): string {
  const target = blueprint.result?.target;
  if (!target) return "Target unavailable";
  return target.id ? `${target.kind} · ${target.id}` : target.kind;
}

export function pipelineModel(blueprint: BlueprintSnapshot): PipelineView {
  const trigger = blueprint.trigger;
  const steps = blueprint.steps ?? [];
  const current = currentStepIndex(blueprint);
  const nodes: PipelineViewNode[] = [
    {
      key: "trigger",
      kind: "trigger",
      title: "Trigger",
      state: blueprint.state,
      current: false,
      summary: trigger?.summary ?? "Trigger details unavailable",
      lastEventAtMs: blueprint.last_event_at_ms ?? null,
      resultSummary: null,
      outcome: null,
      targetSummary: null,
    },
    ...steps.map((step) => ({
      key: `step-${step.index}`,
      kind: "step" as const,
      title: step.title || step.name,
      state: step.state,
      current: current === step.index,
      summary: step.name,
      lastEventAtMs: step.ended_at_ms ?? step.started_at_ms ?? blueprint.last_event_at_ms ?? null,
      resultSummary: step.result_summary ?? step.reason ?? null,
      outcome: step.outcome ?? null,
      targetSummary: null,
    })),
    {
      key: "result",
      kind: "result",
      title: "Result",
      state: blueprint.result?.state ?? "waiting",
      current: false,
      summary: blueprint.result?.reason ?? "",
      lastEventAtMs: blueprint.last_event_at_ms ?? null,
      resultSummary: null,
      outcome: null,
      targetSummary: targetSummary(blueprint),
    },
  ];
  const edges = nodes.slice(0, -1).map((node, index) => ({
    from: node.key,
    to: nodes[index + 1].key,
    label: node.kind === "step" ? node.outcome : null,
  }));
  return {
    id: blueprint.id,
    name: blueprint.name,
    owner: normalizedOwner(blueprint.owner),
    state: blueprint.state,
    nodes,
    edges,
  };
}

function legacyStepState(state: string | null): BlueprintStepState {
  switch (state?.toLowerCase()) {
    case "active":
    case "running":
    case "finishing": return "running";
    case "failed": return "failed";
    case "stopped":
    case "expired": return "done";
    case "skipped": return "skipped";
    default: return "waiting";
  }
}

function legacyState(state: string | null): BlueprintSnapshot["state"] {
  switch (state?.toLowerCase()) {
    case "active":
    case "running":
    case "finishing": return "running";
    case "paused":
    case "held": return "held";
    case "failed": return "failed";
    case "stopped":
    case "expired": return "stopped";
    default: return "idle";
  }
}

export function legacyScenarioPipeline(graph: AutomationGraph, scenario: AutomationScenario): PipelineView {
  const root = graph.nodes.find((node) => automationKey(node.identity) === automationKey(scenario.root));
  const state = legacyState(root?.state ?? null);
  const stepState = legacyStepState(root?.state ?? null);
  const owner: BlueprintOwner = {
    kind: root?.identity.team_id ? "team" : "tab",
    tab_id: root?.identity.owner_tab_id ?? null,
    team_id: root?.identity.team_id ?? null,
  };
  const buckets = root?.outcomes.map((outcome) => outcome.bucket).join(" · ") ?? "";
  const routes = root?.outcomes.flatMap((outcome) => outcome.handlers.map((handler) => `${outcome.bucket}: ${handler.label ?? handler.type}`)) ?? [];
  const schedule = root?.schedule ?? (root?.cadence_ms ? `every ${root.cadence_ms}ms` : null);
  const handlerResults = root?.last_result?.handlers ?? [];
  const resultState: BlueprintResultState = handlerResults.length === 0
    ? "waiting"
    : handlerResults.some((handler) => handler.status === "failed")
      ? "failed"
      : handlerResults.every((handler) => handler.status === "success")
        ? "delivered"
        : "delivering";
  const snapshot: BlueprintSnapshot = {
    id: scenario.id,
    name: scenario.name,
    owner,
    state,
    run_id: null,
    current_step: stepState === "running" ? 0 : null,
    last_event_at_ms: root?.last_result?.accepted_at ?? null,
    trigger: schedule
      ? { type: "schedule", summary: schedule }
      : { type: "event", summary: root?.sentinel_definition ? "Observe tab" : "Observe" },
    steps: [
      {
        index: 0,
        name: root?.name ?? "Probe",
        kind: "command",
        title: root?.sentinel_definition ? "Observe" : "Probe",
        state: stepState,
        outcome: null,
        started_at_ms: null,
        ended_at_ms: root?.last_result?.accepted_at ?? null,
        result_summary: root?.purpose ?? null,
        reason: null,
      },
      {
        index: 1,
        name: "Ask",
        kind: "ask",
        title: buckets ? `Ask · ${buckets}` : "Ask",
        state: root?.last_result ? "done" : "waiting",
        outcome: root?.last_result?.bucket ?? null,
        started_at_ms: null,
        ended_at_ms: root?.last_result?.accepted_at ?? null,
        result_summary: root?.last_result?.bucket ?? null,
        reason: null,
      },
    ],
    result: {
      target: { kind: "notify", id: null },
      state: resultState,
      reason: null,
      body_ref: null,
    },
  };
  const pipeline = pipelineModel(snapshot);
  const resultSummary = routes.length > 0 ? `Routes · ${routes.join(" · ")}` : "Routes unavailable";
  return {
    ...pipeline,
    nodes: pipeline.nodes.map((node) => node.kind === "result" ? { ...node, targetSummary: resultSummary } : node),
  };
}

export function groupBlueprintsByOwner(blueprints: readonly PipelineView[]): PipelineOwnerGroup[] {
  const groups = new Map<string, { kind: PipelineOwnerGroup["kind"]; ownerId: string | null; blueprints: PipelineView[] }>();
  for (const blueprint of blueprints) {
    const kind = blueprint.owner.kind;
    const ownerId = kind === "team" ? blueprint.owner.team_id : blueprint.owner.tab_id;
    const groupKind = ownerId ? kind : "unbound";
    const key = `${groupKind}:${ownerId ?? ""}`;
    const group = groups.get(key) ?? { kind: groupKind, ownerId, blueprints: [] };
    group.blueprints.push(blueprint);
    groups.set(key, group);
  }
  return [...groups.entries()].map(([key, group]) => ({ key, ...group }));
}
