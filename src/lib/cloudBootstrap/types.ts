export type WarmBootstrapStatus =
  | "applied"
  | "partial"
  | "already_applied"
  | "skipped_restored_state"
  | "rejected"
  | "failed";

export type WarmBootstrapRejectReason =
  | "unsupported_schema"
  | "wrong_account"
  | "installation_mismatch"
  | "stale_generation"
  | "target_not_empty"
  | "oversized_projection"
  | "invalid_provenance"
  | "readback_unverified";

export type ProviderSignInState = "ready" | "sign_in_required" | "unsupported";

export interface SanitizedExclusion {
  category: string;
  reason: string;
}

export interface WarmBootstrapAppliedCounts {
  identity_fields: number;
  wake_up_items: number;
  rules: number;
  skills: number;
  preference_fields: number;
}

export interface ProviderSignInCardStatus {
  provider_id: string;
  profile_id?: string | null;
  display_name: string;
  state: ProviderSignInState;
  supported_methods: string[];
  code_input: boolean;
  login_tab_id?: string | null;
  requires_owner_input: boolean;
  unsupported_reason?: string | null;
}

export interface CloudCreatePhases {
  requested_at_ms?: number | null;
  provisioned_at_ms?: number | null;
  enrolled_at_ms?: number | null;
  peer_ready_at_ms?: number | null;
  seed_started_at_ms?: number | null;
  warm_console_ready_at_ms?: number | null;
  warm_bootstrap_delta_secs?: number | null;
  within_budget?: boolean | null;
  authoritative_backend_phases?: boolean;
  provider_signin_started_at_ms?: number | null;
  provider_signin_completed_at_ms?: number | null;
}

export interface CloudWarmBootstrapSummary {
  schema_version: number;
  status: WarmBootstrapStatus;
  reject_reason?: WarmBootstrapRejectReason | null;
  recovery_revision?: string | null;
  creation_generation: string;
  readable_owner_context: boolean;
  wake_up_verified: boolean;
  applied_counts: WarmBootstrapAppliedCounts;
  excluded: SanitizedExclusion[];
  warnings: string[];
  general_tab_id?: string | null;
  providers: ProviderSignInCardStatus[];
}

export interface CloudBootstrapSnapshot {
  machine_id: string;
  name: string;
  state: string;
  peer_id: string;
  installation_id?: string | null;
  creation_generation?: string | null;
  recovery_revision?: string | null;
  create_phases?: CloudCreatePhases | null;
  warm_bootstrap?: CloudWarmBootstrapSummary | null;
}

export interface PrepareProviderSignInItem {
  provider_id: string;
  profile_id?: string | null;
  display_name: string;
  state: ProviderSignInState;
  supported_methods: string[];
  code_input: boolean;
  tab_id?: string | null;
  remote_tab_id?: string | null;
  card_ready: boolean;
  requires_owner_input: boolean;
  unsupported_reason?: string | null;
}

export interface PrepareProviderSignInResponse {
  providers: PrepareProviderSignInItem[];
}
