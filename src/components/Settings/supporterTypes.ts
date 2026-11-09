export interface SupporterTokenStatus {
  adapter: string;
  token_mode: "required" | "optional" | "none";
  present: boolean;
  path?: string;
  mode_ok?: boolean | null;
}

export interface SupporterAdapter {
  id: string;
  token_mode: SupporterTokenStatus["token_mode"];
  primitives: { choice: boolean; noul: boolean; score: boolean };
  models: { id: string; display_name: string }[];
  token?: SupporterTokenStatus;
  pricing?: SupporterStatus["pricing"];
  metadata?: SupporterMetadata;
}

export type SupporterSpendTotals = import("../../../gen/SupporterSpendTotals").SupporterSpendTotals;

export interface SupporterSpend {
  today: SupporterSpendTotals;
  month: SupporterSpendTotals;
  exhausted: boolean;
  next_probe_at: number | null;
  storage_ready: boolean;
}

export interface SupporterStatus {
  state:
    | "active"
    | "credit_exhausted"
    | "budget_reached"
    | "not_connected"
    | "no_token"
    | "token_unusable"
    | "adapter_unavailable";
  token_mode: SupporterTokenStatus["token_mode"];
  adapter: string | null;
  model: string | null;
  deadline_ms: number;
  connected: boolean;
  last_activity: number | null;
  calls_last_hour: number;
  calls_last_day: number;
  last_verdict: Partial<
    Record<"rerank" | "tab_state", { time: number; label: string | null; confidence: number | null }>
  >;
  budget: {
    reached: boolean;
    calls_per_hour: number | null;
    free_at: number | null;
    storage_ready: boolean;
  };
  pricing?: {
    display_name: string;
    input_usd_per_million: number;
    output_usd_per_million: number;
  } | null;
  spend: SupporterSpend;
  token: SupporterTokenStatus;
  adapter_registered: boolean;
  adapters: SupporterAdapter[];
  connections: SupporterConnection[];
}

export type SupporterMetadata = Partial<import("../../../gen/DecisionMetadata").DecisionMetadata>;

export type SupporterFailure = import("../../../gen/SupporterFailure").SupporterFailure;
export interface SupporterConnection {
  adapter: string;
  model: string | null;
  active: boolean;
  token_mode: SupporterTokenStatus["token_mode"] | null;
  token: { present: boolean; mode_ok?: boolean | null };
  pricing?: SupporterStatus["pricing"];
  metadata?: SupporterMetadata | null;
  error?: SupporterFailure | null;
}

export interface SupporterMutation {
  state?: "active" | "saved_not_active";
  adapter?: string;
  error?: SupporterFailure;
}
