import { daemonFetch } from "../../../lib/webApi";
import { PlanReadError } from "../plansApi";
import { authHeaders } from "../../MemoryPanel/memoryApi";
import type { ApiConfig } from "../../../lib/types";
import type { PlanView as Plan } from "../planProjection";

export type PlanTaskCounts = import("../../../../gen/TaskCounts").TaskCounts;

export type HeldReview = import("../../../../gen/HeldReview").HeldReview;

export type OpenDecisionGate = import("../../../../gen/OpenDecisionGate").OpenDecisionGate;

export type HeldForDecision = import("../../../../gen/HeldForDecision").HeldForDecision;

export type BounceSummary = import("../../../../gen/BounceSummary").BounceSummary;

export type PlanEvent = import("../../../../gen/PlanEvent").PlanEvent;

export type TeamMissionControlEvent = import("../../../../gen/TeamMissionControlEvent").TeamMissionControlEvent;

export type EventLog = import("../../../../gen/EventLog").EventLog;

export type AreaFacetState = import("../../../../gen/AreaFacetState").AreaFacetState;

export type AreaGroup = import("../../../../gen/AreaGroup").AreaGroup;

export type RoleGroup = import("../../../../gen/RoleGroup").RoleGroup;

export type AreaGrouping = import("../../../../gen/AreaGrouping").AreaGrouping;

export type KpiLedgerState = import("../../../../gen/KpiLedgerState").KpiLedgerState;
export type KpiNotRecordingReason = import("../../../../gen/KpiNotRecordingReason").KpiNotRecordingReason;

export type PlanKpiTokens = import("../../../../gen/PlanKpiTokens").PlanKpiTokens;

export type PlanKpiSpend = import("../../../../gen/PlanKpiSpend").PlanKpiSpend;

export type MissionControlKpi = import("../../../../gen/MissionControlKpi").MissionControlKpi;

export type SpendScope = import("../../../../gen/SpendScope").SpendScope;

/** Provider/window allowance already consumed. Never a per-model figure. */
export type ProviderAllowance = import("../../../../gen/MissionControlProviderAllowance").MissionControlProviderAllowance;

export type ModelSpend = import("../../../../gen/ModelSpend").ModelSpend;

export type ProviderSpend = import("../../../../gen/ProviderSpend").ProviderSpend;

export type MissionControlSpend = import("../../../../gen/MissionControlSpend").MissionControlSpend;

export type MissionControlState = import("../../../../gen/MissionControl").MissionControl;

export interface PlanBoardState {
  revision?: string;
  control_loop?: import("../planControlLoop").PlanControlLoop;
  plan_id: string;
  task_counts: PlanTaskCounts;
  mission_control: Pick<MissionControlState, "bounces" | "event_log" | "spend"> & Partial<Omit<MissionControlState, "bounces" | "event_log" | "spend">>;
  next_event_cursor?: number | null;
  event_window_loaded?: boolean;
}

export type MissionControlPlanView = Plan | TeamPlanSummary;

export type TeamPlanSummary = import("../../../../gen/TeamPlanSummary").TeamPlanSummary;

export type TeamMissionControlState = import("../../../../gen/TeamMissionControlState").TeamMissionControlState;

export type TeamMissionControlQuery = Partial<{ [K in keyof import("../../../../gen/TeamMissionControlQuery").TeamMissionControlQuery]: NonNullable<import("../../../../gen/TeamMissionControlQuery").TeamMissionControlQuery[K]> }>;

export async function fetchPlanBoardState(
  api: ApiConfig,
  planId: string,
  signal?: AbortSignal,
  eventCursor?: number,
  includeEvents = true,
): Promise<PlanBoardState> {
  const query=new URLSearchParams();
  if(eventCursor!==undefined)query.set("event_cursor",String(eventCursor));
  if(!includeEvents)query.set("events","false");
  const res = await daemonFetch(api, `/api/plans/${encodeURIComponent(planId)}/board-state${query.size ? `?${query}` : ""}`, {
    headers: authHeaders(api),
    signal,
  });
  if (!res.ok) throw new PlanReadError(res.status, `plan state failed: ${res.status} ${await res.text()}`);
  const body = (await res.json()) as PlanBoardState;
  return body;
}

export async function fetchTeamMissionControlState(
  api: ApiConfig,
  teamId: string,
  query: TeamMissionControlQuery = {},
  signal?: AbortSignal,
): Promise<TeamMissionControlState> {
  const params = new URLSearchParams();
  if (query.limit !== undefined) params.set("limit", String(query.limit));
  if (query.cursor !== undefined) params.set("cursor", query.cursor);
  const suffix = params.toString();
  const res = await daemonFetch(
    api,
    `/api/teams/${encodeURIComponent(teamId)}/mission-control/state${suffix ? `?${suffix}` : ""}`,
    { headers: authHeaders(api), signal },
  );
  if (!res.ok) throw new Error(`team mission control state failed: ${res.status} ${await res.text()}`);
  return (await res.json()) as TeamMissionControlState;
}
