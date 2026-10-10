import type { GraphRun, RunEvent } from "../../lib/blueprintRun";
import { nodeKeys, type BlueprintModel } from "./blueprintModel";
import { runTransitions } from "./blueprintRunModel";

export interface BlueprintReplayOutcome {
  nodeKey: string;
  failed: boolean;
  noAction: boolean;
  reason: string;
  label: string;
}

const FAILURE = new Set(["error", "failed", "nonzero", "cancelled", "deadline"]);
const QUIET = new Set(["no action", "skipped", "noop"]);
const failure = (value: string) => FAILURE.has(value.toLowerCase()) || value.toLowerCase().endsWith("_failed");

function recordedResult(model: BlueprintModel, event: RunEvent) {
  return model.nodes.find((node) => node.stageKey === event.stage)?.stage.history
    ?.find((transition) => transition.id === event.transition)?.handlers.find((handler) => handler.handler_id === event.handler);
}

export function blueprintReplayOutcome(model: BlueprintModel, run: GraphRun): BlueprintReplayOutcome | null {
  const transitions = runTransitions(run);
  const last = transitions[transitions.length - 1];
  if (!last) return null;
  const failedEvent = [...run.events].reverse().filter((event) => !event.handler || !QUIET.has(event.status.toLowerCase())).find((event) => {
    const result = recordedResult(model, event);
    return event.stage && (failure(event.outcome) || failure(event.status) || failure(result?.status ?? "") || failure(result?.command_result?.status ?? ""));
  }) ?? (failure(last.bucket) ? last.events[0] : undefined);
  const actions = last.events.filter((event) => event.handler && !QUIET.has(event.status.toLowerCase()));
  const event = failedEvent ?? actions[actions.length - 1] ?? last.events[0];
  const keys = nodeKeys(event.stage!);
  const action = event.handler && !QUIET.has(event.status.toLowerCase()) ? model.nodes.find((node) => node.key === keys.action(event.outcome, event.handler!)) : undefined;
  const node = action ?? model.nodes.find((node) => node.key === keys.decision);
  if (!node) return null;
  const result = recordedResult(model, event);
  const failed = failedEvent != null;
  const noAction = !action;
  const reason = result?.summary?.trim() || event.reason?.trim() || (failure(event.status) ? event.status : result?.command_result?.status && failure(result.command_result.status) ? result.command_result.status : event.outcome);
  const title = `${node.title}${node.subtitle ? ` · ${node.subtitle}` : ""}`;
  return {
    nodeKey: node.key, failed, noAction, reason,
    label: failed ? `Stopped at ${title}: ${reason}` : noAction ? `Finished: ${event.outcome}, no action` : `Finished at ${title}`,
  };
}
