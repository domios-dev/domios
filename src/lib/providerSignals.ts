import type {ProbeSnapshot} from "./providerCapabilities";

export type ProviderErrorKind = "rate_limit" | "usage_limit" | "server_overloaded" | "auth_expiry" | "authentication" | "other";

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

export interface ProviderSignals {
  capabilities?: ProbeSnapshot | null;
  auth: {loggedIn: boolean; authMethod: string | null; subscriptionType: string | null; configDirectory: string | null} | null;
  api_error: ProviderErrorKind | null;
  permission_mode: "default" | "acceptEdits" | "bypassPermissions" | "plan" | "dontAsk" | "auto" | null;
}
