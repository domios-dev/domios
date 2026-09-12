// TS mirror of the Rust Mission Control projection
// (src-tauri/crates/codeterm-plan/src/service_plan/state/mission_control.rs) plus
// the task counts the same snapshot carries. Field names match the JSON wire shape.

import { authHeaders, baseUrl } from "../../MemoryPanel/memoryApi";
import type { ApiConfig } from "../../../lib/types";
import type { GateStatus } from "../types";

export interface PlanTaskCounts {
  pending: number;
  in_progress: number;
  in_review: number;
  blocked: number;
  paused: number;
  done: number;
  cancelled: number;
  total: number;
}

export interface HeldReview {
  task_id: string;
  name: string;
  verdicts: number;
  last_verdict_at: number | null;
  age_ms: number | null;
}

export interface OpenDecisionGate {
  id: string;
  question: string;
  status: GateStatus;
  asked_at: number;
  age_ms: number;
  blocking_task_count: number;
}

export interface HeldForDecision {
  held_reviews: HeldReview[];
  open_gates: OpenDecisionGate[];
  oldest_hold_age_ms: number | null;
  verdicts_on_held: number;
}

export interface BounceSummary {
  total_attempts: number;
  tasks_on_second_attempt: number;
  first_try_done: number;
  labelled_bounced: number;
}

export interface PlanEvent {
  ts: number;
  /** Last event folded into this line; equals `ts` when `count` is 1. */
  last_ts: number;
  count: number;
  kind: string;
  author: string;
  author_tab_id: string | null;
  task_id: string | null;
  task_name: string | null;
  text: string;
}

export interface EventLog {
  recorded_events: number;
  truncated: boolean;
  entries: PlanEvent[];
}

export type AreaFacetState = "declared" | "no_facet";

export interface AreaGroup {
  value: string;
  label: string;
  task_count: number;
  task_ids: string[];
}

export interface RoleGroup {
  /** `null` is the stored absence of an executor role, never a guess. */
  executor_role: string | null;
  task_count: number;
  task_ids: string[];
}

export interface AreaGrouping {
  state: AreaFacetState;
  facet_key: string | null;
  facet_label: string | null;
  groups: AreaGroup[];
  untagged_task_count: number;
  role_fallback: RoleGroup[];
}

export type KpiLedgerState = "recording" | "not_recording";
export type KpiNotRecordingReason = "disabled" | "no_rows_for_plan" | "unreadable" | "not_read";

export interface PlanKpiTokens {
  input: number;
  uncached_input: number;
  output: number;
  total: number;
}

export interface PlanKpiSpend {
  events: number;
  bytes: number;
  turns: number;
  tokens: PlanKpiTokens;
}

export interface MissionControlKpi {
  state: KpiLedgerState;
  not_recording_reason: KpiNotRecordingReason | null;
  spend: PlanKpiSpend | null;
  /** Config key that turns recording back on; non-null only for the opt-out state. */
  enable_setting: string | null;
}

export type SpendScope = "machine";

/** Provider/window allowance already consumed. Never a per-model figure. */
export interface ProviderAllowance {
  captured_at: number;
  session_used_pct: number | null;
  session_resets_at: number | null;
  weekly_used_pct: number | null;
  weekly_resets_at: number | null;
  stale_reason: string | null;
}

export interface ModelSpend {
  /** `null` is the ledger's unresolved model identity, shown as its own row. */
  model: string | null;
  turns: number;
  input_tokens: number;
  uncached_input_tokens: number;
  output_tokens: number;
  /** Share of the machine's measured token total; never a share of allowance. */
  token_share_pct: number;
}

export interface ProviderSpend {
  provider: string;
  label: string;
  allowance: ProviderAllowance | null;
  turns: number;
  input_tokens: number;
  uncached_input_tokens: number;
  output_tokens: number;
  models: ModelSpend[];
}

export interface MissionControlSpend {
  scope: SpendScope;
  state: KpiLedgerState;
  not_recording_reason: KpiNotRecordingReason | null;
  enable_setting: string | null;
  measured_tokens: number;
  turns: number;
  providers: ProviderSpend[];
}

export interface MissionControlState {
  held: HeldForDecision;
  bounces: BounceSummary;
  event_log: EventLog;
  areas: AreaGrouping;
  kpi: MissionControlKpi;
  spend: MissionControlSpend;
}

export interface PlanBoardState {
  plan_id: string;
  task_counts: PlanTaskCounts;
  mission_control: MissionControlState;
}

export async function fetchPlanBoardState(
  api: ApiConfig,
  planId: string,
  signal?: AbortSignal,
): Promise<PlanBoardState> {
  const res = await fetch(`${baseUrl(api)}/api/plans/${encodeURIComponent(planId)}/state`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new Error(`plan state failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as PlanBoardState;
  return body;
}
