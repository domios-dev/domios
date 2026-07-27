import { invoke } from "./bridge";
export { canMakeOrchestrator } from "./orchestratorOnboardEligibility";

export async function makeOrchestrator(tabId: string): Promise<void> {
  await invoke("mux_make_orchestrator", { tabId });
}

export async function makePlanner(tabId: string, planId: string): Promise<void> {
  await invoke("mux_make_planner", { tabId, planId });
}
