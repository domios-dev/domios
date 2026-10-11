import { invoke } from "./bridge";
import { teamCallerClaim } from "./teamActions";
export { canMakeOrchestrator } from "./orchestratorOnboardEligibility";

export async function makeOrchestrator(tabId: string, sessionBindingNonce?: string): Promise<void> {
  await invoke("mux_make_orchestrator", { tabId, ...teamCallerClaim(tabId, sessionBindingNonce) });
}

export async function makePlanner(tabId: string, planId: string, sessionBindingNonce?: string): Promise<void> {
  await invoke("mux_make_planner", { tabId, planId, ...teamCallerClaim(tabId, sessionBindingNonce) });
}
