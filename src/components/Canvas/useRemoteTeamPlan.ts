import { useEffect, useMemo, useState } from "react";
import { formatPlanFetchError } from "../../lib/fetchResilience";
import { peerEventRail } from "../../lib/peerEventRail";
import type { ApiConfig } from "../../lib/types";
import type { TeamMissionControlState } from "./missionControl/missionControlState";
import { readRemoteTeamPlan, remotePlanApi, teamPlanLookupChanged, type RemoteTeamPlanTarget } from "./remoteTeamPlanModel";

export function useRemoteTeamPlan(api: ApiConfig, target: RemoteTeamPlanTarget) {
  const { peerId } = target;
  const peerApi = useMemo(() => remotePlanApi(api, peerId), [api, peerId]);
  const [plan, setPlan] = useState<TeamMissionControlState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [request, retry] = useState(0);
  useEffect(() => {
    let controller: AbortController | null = null;
    let activePlanId: string | undefined;
    let queued = false;
    let mounted = true;
    const read = () => {
      if (controller) {
        queued = true;
        return;
      }
      const current = new AbortController();
      controller = current;
      setError(null);
      void readRemoteTeamPlan(api, target, current.signal).then((result) => {
        if (current.signal.aborted) return;
        activePlanId = result.active_plan?.id;
        setPlan(result);
      }).catch((cause: unknown) => {
        if (current.signal.aborted) return;
        activePlanId = undefined;
        setPlan(null);
        setError(formatPlanFetchError(cause));
      }).finally(() => {
        if (controller !== current) return;
        controller = null;
        if (mounted && queued) {
          queued = false;
          read();
        }
      });
    };
    const stop = peerEventRail.observe(peerId, (event) => {
      if (event.type === "invalidate") {
        controller?.abort();
        controller = null;
        queued = false;
        activePlanId = undefined;
        setPlan(null);
        setError("The peer connection changed. Waiting for its Plan and access grant.");
      } else if (teamPlanLookupChanged(event, activePlanId)) read();
    });
    read();
    return () => {
      mounted = false;
      controller?.abort();
      stop();
    };
  }, [api, target, peerId, request]);
  return { plan, error, peerApi, loading: !plan && !error, retry: () => retry(value => value + 1) };
}
