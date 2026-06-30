import { invoke } from "./bridge";
export { canMakeOrchestrator } from "./orchestratorOnboardEligibility";

export async function makeOrchestrator(paneId: string): Promise<void> {
  await invoke("mux_make_orchestrator", { paneId });
}
