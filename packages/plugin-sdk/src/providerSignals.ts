export interface ProviderAuthStatus {
  loggedIn: boolean;
  authMethod: string | null;
  subscriptionType: string | null;
  configDirectory: string | null;
}

export interface ProviderSessionObservations {
  unavailable_model?: string;
  api_error: "rate_limit" | "usage_limit" | "server_overloaded" | "auth_expiry" | "authentication" | "other" | null;
  permission_mode: "default" | "acceptEdits" | "bypassPermissions" | "plan" | "dontAsk" | "auto" | null;
  clears_api_error: boolean;
  retry_after_secs?: number;
  turn_id?: string;
  retrying?: boolean;
}
