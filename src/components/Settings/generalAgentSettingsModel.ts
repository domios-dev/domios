import type { GeneralAgentConfig } from "../../lib/types";

export function capabilityHintsEnabled(config: Partial<GeneralAgentConfig> | undefined): boolean {
  return config?.capability_hints ?? true;
}

export function capabilityHintsPatch(enabled: boolean): Partial<GeneralAgentConfig> {
  return { capability_hints: enabled };
}
