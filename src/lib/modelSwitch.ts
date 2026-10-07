import type { ProviderDescriptor } from "./types";

export function supportsLiveModelSwitch(descriptor: ProviderDescriptor | null | undefined): boolean {
  return modelSwitchModeSupported(descriptor?.model_switch);
}

export function modelSwitchLabel(mode: string | undefined): string {
  if (mode === "next_turn") return "Next turn";
  if (mode === "live") return "Live";
  if (mode === "restart_resume") return "Restart/resume";
  return "Unavailable";
}

export function modelSwitchModeSupported(mode: string | undefined): boolean {
  return mode === "restart_resume" || mode === "live" || mode === "next_turn";
}
