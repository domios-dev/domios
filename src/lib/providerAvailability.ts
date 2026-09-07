import type { ProviderAvailability, ProviderDescriptor } from "./types";

const KNOWN: readonly ProviderAvailability[] = ["available", "unavailable", "unknown"];

/**
 * The one availability answer every surface renders, so `codeterm agent providers`
 * and the picker cannot disagree. `unknown` is what a probe that could not run
 * reports, and what a descriptor from a peer that predates the field means.
 */
export function providerAvailability(
  provider: Pick<ProviderDescriptor, "availability">,
): ProviderAvailability {
  const value = provider.availability;
  return value && KNOWN.includes(value) ? value : "unknown";
}

/** Only the observed negative blocks a launch; an unanswered probe does not. */
export function isProviderSelectable(
  provider: Pick<ProviderDescriptor, "availability">,
): boolean {
  return providerAvailability(provider) !== "unavailable";
}

export function providerAvailabilityLabel(
  provider: Pick<ProviderDescriptor, "availability">,
): string {
  switch (providerAvailability(provider)) {
    case "available":
      return "installed";
    case "unavailable":
      return "not installed";
    default:
      return "availability unknown";
  }
}
