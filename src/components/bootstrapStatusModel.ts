import type { StartupPhase } from "../lib/serverReadiness";
import { brandStateModel, type BrandStateModel } from "./brand/brandStateModel";

const startupPhaseLabels: Record<StartupPhase, string> = {
  listener: "Opening API listener",
  integrity: "Verifying resources",
  states: "Initializing state",
  providers: "Registering providers",
  service: "Preparing service",
  plans: "Loading plans",
  background: "Starting background services",
  platform: "Preparing platform",
};

export function bootstrapStatusModel(failed: boolean, phase: StartupPhase | null): BrandStateModel {
  if (failed) {
    return brandStateModel({ error: { title: "Domios could not finish starting", sentence: "The server did not become ready. Restart Domios to try again." } });
  }
  return brandStateModel({ loading: true, label: "Starting Domios", detail: phase ? startupPhaseLabels[phase] : undefined });
}
