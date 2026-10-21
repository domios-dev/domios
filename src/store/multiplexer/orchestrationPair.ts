import { freeLimitGate, precheckFreeLimit, raiseFreeLimit } from "../../components/billing/freeLimitGate";
import { freeLimitBlocked } from "../../components/billing/freeLimitModel";
import type { OrchestrationPairInput, OrchestrationPairResult } from "../../lib/types";

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

function orchestrationPairParams(input: OrchestrationPairInput) {
  return {
    orchestrator_agent: input.orchestratorAgent,
    orchestrator_model: input.orchestratorModel,
    orchestrator_reasoning_effort: input.orchestratorReasoningEffort,
    planner_agent: input.plannerAgent,
    planner_model: input.plannerModel,
    planner_reasoning_effort: input.plannerReasoningEffort,
    workspace_id: input.workspaceId,
    title: input.title,
    existing_tab_id: input.existingTabId,
  };
}

/** A pair mints a Team and up to two tabs; a full limit opens the plans modal and rejects with the coded refusal. */
export async function requestOrchestrationPair(invoke: Invoke, input: OrchestrationPairInput): Promise<OrchestrationPairResult> {
  if (precheckFreeLimit(["teams", "agent.tabs"])) throw freeLimitBlocked(freeLimitGate.prompt() ?? "teams");
  try {
    return await invoke<OrchestrationPairResult>("mux_create_orchestration_pair", { params: orchestrationPairParams(input) });
  } catch (error) {
    raiseFreeLimit(error, "teams");
    throw error;
  }
}
