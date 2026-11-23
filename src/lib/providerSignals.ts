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
type BackgroundRoster = import("../../gen/BackgroundRoster").BackgroundRoster;
type OptionalRosterFields = "unknown" | "ended" | "kinds";
export type ProviderBackgroundRoster = Omit<BackgroundRoster, OptionalRosterFields> & Partial<Pick<BackgroundRoster, OptionalRosterFields>>;
export type ProviderSignals = Omit<import("../../gen/ProviderSignals").ProviderSignals, "background" | "capabilities"> & { background?: ProviderBackgroundRoster; capabilities?: ProbeSnapshot | null };
