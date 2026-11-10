import type {ProbeSnapshot} from "./providerCapabilities";
export type ProviderErrorKind = import("../../gen/ApiErrorKind").ApiErrorKind;

const providerErrorLabels: Record<ProviderErrorKind, string> = {
  rate_limit: "Rate limited",
  usage_limit: "Usage limit reached",
  server_overloaded: "Provider overloaded",
  auth_expiry: "Sign-in expired",
  authentication: "Sign in to continue",
  other: "Provider error",
};

export function providerErrorReason(error: ProviderErrorKind | null | undefined): string | null {
  return error ? providerErrorLabels[error] : null;
}
export type ProviderSignals = Omit<import("../../gen/ProviderSignals").ProviderSignals, "background" | "capabilities"> & { background?: Omit<import("../../gen/BackgroundRoster").BackgroundRoster, "unknown" | "ended"> & Partial<Pick<import("../../gen/BackgroundRoster").BackgroundRoster, "unknown" | "ended">>; capabilities?: ProbeSnapshot | null };
