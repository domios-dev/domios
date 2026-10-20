export interface OrchestrationPairInput {
  orchestratorAgent: string;
  orchestratorModel?: string;
  orchestratorReasoningEffort?: string;
  plannerAgent: string;
  plannerModel?: string;
  plannerReasoningEffort?: string;
  workspaceId?: string;
  title?: string;
  existingTabId?: string;
}

export interface OrchestrationPairResult {
  plan_id: string;
  orchestrator_tab_id: string;
  orchestrator_model?: string;
  planner_tab_id: string;
  planner_model?: string;
  warning?: string;
}
