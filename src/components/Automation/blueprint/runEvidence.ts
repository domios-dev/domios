import type { BlueprintDocument, BlueprintIrNode } from "../../../lib/blueprintApi";
import type { PipelineView } from "../pipelineModel";
import type { AutomationGraph, AutomationTransition, BlueprintSnapshot } from "../types";

export interface StepEvidence {
  step: string;
  state: string;
  outcome: string | null;
  summary: string | null;
  at: number | null;
}

/** One retained watch run as per-step evidence, in the order the steps acted. */
export interface RetainedRun {
  id: string;
  at: number;
  state: string;
  incomplete: boolean;
  steps: readonly StepEvidence[];
}

function decisionStep(document: BlueprintDocument, bucket: string): BlueprintIrNode | undefined {
  const steps = document.steps.slice(1);
  return steps.find((step) => step.buckets?.includes(bucket) || step.outcomes?.includes(bucket) || Object.prototype.hasOwnProperty.call(step.routes ?? {}, bucket))
    ?? steps.find((step) => step.buckets?.length);
}

function handlerState(status: string | null): string {
  return status === "completed" ? "done" : status ?? "unknown";
}

export function transitionEvidence(document: BlueprintDocument, transition: AutomationTransition): StepEvidence[] {
  const source = document.steps[0];
  const decision = decisionStep(document, transition.bucket);
  const decidedBy = transition.decided_by === "pattern" ? "matched a pattern" : transition.decided_by === "adapter" ? "classified by a model" : null;
  const evidence: StepEvidence[] = [];
  if (source) evidence.push({ step: source.id, state: "done", outcome: null, summary: "Reported new evidence", at: transition.accepted_at });
  if (decision && decision !== source) evidence.push({ step: decision.id, state: "done", outcome: transition.bucket, summary: transition.reason ?? decidedBy, at: transition.accepted_at });
  for (const handler of transition.handlers) {
    if (!document.steps.some((step) => step.id === handler.handler_id)) continue;
    evidence.push({ step: handler.handler_id, state: handlerState(handler.status), outcome: null, summary: handler.summary ?? (handler.command_result?.exit_code != null ? `exit ${handler.command_result.exit_code}` : null), at: handler.accepted_at ?? transition.accepted_at });
  }
  return evidence;
}

export function watchRuns(graph: AutomationGraph, blueprint: BlueprintSnapshot): RetainedRun[] {
  const document = blueprint.call_graph;
  if (document?.kind !== "watch") return [];
  const history = new Map(graph.nodes.filter((node) => node.identity.kind === "blueprint" && node.identity.id === blueprint.id)
    .flatMap((node) => node.history ?? []).map((transition) => [transition.id, transition]));
  return (graph.runs ?? []).filter((run) => run.root_node === blueprint.id).flatMap((run) => {
    const transition = history.get(run.root_transition);
    if (!transition) return [];
    return [{ id: run.id, at: run.started_at, state: run.state, incomplete: run.incomplete, steps: transitionEvidence(document, transition) }];
  }).sort((a, b) => b.at - a.at);
}

export function withRetainedRuns(pipeline: PipelineView, retained: readonly RetainedRun[]): PipelineView {
  if (!retained.length || pipeline.runs?.length) return pipeline;
  const runs = retained.map((run) => ({ id: run.id, at: run.at, state: run.state, pipeline: { ...pipeline, runs: undefined, runId: run.id, retained: run } }));
  return { ...pipeline, runs, runId: retained[0].id, retained: retained[0] };
}
