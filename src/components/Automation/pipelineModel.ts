import { type BlueprintOwner, type BlueprintSnapshot } from "./types";

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
  runs?: readonly { id: string; at: number; state: string; pipeline: PipelineView }[];
  id: string;
  name: string;
  owner: BlueprintOwner;
  state: string;
  nodes: readonly PipelineViewNode[];
  edges: readonly PipelineViewEdge[];
}

function normalizedOwner(owner: BlueprintOwner | undefined): BlueprintOwner {
  return owner ?? { kind: "tab", tab_id: null, team_id: null };
}

function currentStepIndex(blueprint: BlueprintSnapshot): number | null {
  const steps = blueprint.steps ?? [];
  if (blueprint.current_step !== null && blueprint.current_step !== undefined) {
    return steps.some((step) => step.index === blueprint.current_step) ? blueprint.current_step : null;
  }
  return null;
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
    runs: blueprint.runs?.map((run) => ({
      id: run.id, at: run.started_at_ms, state: run.state,
      pipeline: pipelineModel({ ...blueprint, runs: undefined, run_id: run.id,
        state: run.state === "completed" ? "idle" : run.state, current_step: run.current_step,
        last_event_at_ms: run.last_event_at_ms, steps: run.steps, result: run.result }),
    })),
    id: blueprint.id,
    name: blueprint.name,
    owner: normalizedOwner(blueprint.owner),
    state: blueprint.state,
    nodes,
    edges,
  };
}
