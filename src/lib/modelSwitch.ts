import type { ProviderDescriptor } from "./types";

/** A live tab can change model only when its provider declares a mechanism that
 *  leaves the provider's machine-global default alone. */
export function supportsLiveModelSwitch(descriptor: ProviderDescriptor | null | undefined): boolean {
  return descriptor?.model_switch === "restart_resume";
}
