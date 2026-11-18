import type { ProviderDescriptor } from "./types";
import { providerAvailability } from "./providerAvailability";

export function providerCardState(provider: Pick<ProviderDescriptor, "availability" | "availability_checking">) {
  if (provider.availability_checking) return { state: "loading", label: "Checking installation…" } as const;
  const state = providerAvailability(provider);
  return { state, label: state === "available" ? "Installed" : state === "unavailable" ? "Not installed" : "Availability unknown" };
}
