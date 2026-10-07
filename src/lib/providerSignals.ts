import type {ProbeSnapshot} from "./providerCapabilities";
export interface ProviderSignals {
  capabilities?: ProbeSnapshot | null;
  auth: {loggedIn: boolean; authMethod: string | null; subscriptionType: string | null; configDirectory: string | null} | null;
  api_error: "rate_limit" | "authentication" | "other" | null;
  permission_mode: "default" | "acceptEdits" | "bypassPermissions" | "plan" | "dontAsk" | "auto" | null;
}
