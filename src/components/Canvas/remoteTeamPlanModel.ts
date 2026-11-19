import { daemonTransportForPeer } from "../../lib/daemonBootstrap";
import type { ApiConfig } from "../../lib/types";
import { uid } from "../../lib/uid";
import type { PeerEvent } from "../../lib/peerEventRail";
import { fetchTeamMissionControlState, type TeamMissionControlState } from "./missionControl/missionControlState";

export interface RemoteTeamPlanTarget {
  peerId: string;
  teamId: string;
  name: string;
}

export function remotePlanApi(api: ApiConfig, peerId: string): ApiConfig {
  if (!peerId.trim()) throw new Error("An exact peer installation id is required");
  return { ...api, peerId };
}

export async function readRemoteTeamPlan(api: ApiConfig, target: RemoteTeamPlanTarget, signal?: AbortSignal) {
  const state = await fetchTeamMissionControlState(remotePlanApi(api, target.peerId), target.teamId, { limit: 1 }, signal);
  if (state.team_id !== target.teamId || state.team.id !== target.teamId
    || (state.active_plan && state.active_plan.team_id !== target.teamId)) {
    throw new Error("Remote plan lookup did not match the selected Team.");
  }
  return state;
}

export function teamPlanLookupChanged(event: PeerEvent, planId?: string): boolean {
  return event.type === "reconcile" || event.type === "plans-loaded"
    || (event.type === "plan-changed" && (!planId || event.data.reset || event.data.deleted));
}

type RemotePlanAction = "plan.review.accept" | "plan.review.bounce" | "plan.task.dispatch";
type RemoteGrant = TeamMissionControlState["remote_plan_actions"];
export function remotePlanGrantReason(grant: RemoteGrant): string | null {
  if (grant?.unavailable_reason === null && grant.protocol_version !== null
    && grant.protocol_version > 0 && Number.isSafeInteger(grant.protocol_version)) return null;
  return grant?.unavailable_reason || "The owning machine has not confirmed permission to review or dispatch.";
}

export async function runRemotePlanAction(
  target: RemoteTeamPlanTarget,
  grant: RemoteGrant,
  planId: string,
  taskId: string,
  action: RemotePlanAction,
  fields: { note: string } | { execution_intent: "read_only" | "mutating"; workspace_id: string },
) {
  const refusal = remotePlanGrantReason(grant);
  if (!grant || refusal) throw new Error(refusal ?? undefined);
  if (!target.peerId.trim()) throw new Error("An exact peer installation id is required");
  const response = await daemonTransportForPeer(target.peerId).json<{ result?: unknown; error?: { message: string } }>(
    "POST", "/api/mesh/rpc", {
      method: action,
      request_id: uid(),
      protocol_version: grant.protocol_version,
      params: { ...fields, plan_id: planId, task_id: taskId },
    },
  );
  if (response.error) throw new Error(response.error.message);
  if (!("result" in response)) throw new Error("The owning machine did not confirm the request.");
}
