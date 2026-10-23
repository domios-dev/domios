import type { Tab, TabStatus } from "../../lib/types";
import type { ExtendedState } from "./tree";
import { sameJson } from "../../lib/sameJson";

export interface MuxStatusDelta {
  generation: number;
  base_generation: number;
  status_deltas: Record<string, { status: TabStatus | null; agent_state: Tab["agent_state"] }>;
}

export function isMuxStatusDelta(payload: unknown): payload is MuxStatusDelta {
  if (!payload || typeof payload !== "object") return false;
  const delta = payload as Partial<MuxStatusDelta>;
  return Number.isSafeInteger(delta.generation) && Number.isSafeInteger(delta.base_generation)
    && delta.status_deltas != null && typeof delta.status_deltas === "object";
}

export function statusDeltaAction(generation: number | undefined, delta: MuxStatusDelta): "ignore" | "apply" | "refresh" {
  if (generation !== undefined && delta.generation <= generation) return "ignore";
  return generation !== undefined && generation === delta.base_generation && delta.generation > delta.base_generation
    ? "apply" : "refresh";
}

export function applyStatusDelta(state: ExtendedState, delta: MuxStatusDelta): ExtendedState {
  if (state.generation !== delta.base_generation) {
    return state.muxResyncRequired ? state : { ...state, muxResyncRequired: true };
  }
  if (delta.generation <= delta.base_generation) return state;
  let tabs = state.tabs;
  let statuses = state.tabStatuses;
  for (const [id, change] of Object.entries(delta.status_deltas)) {
    const tab = tabs[id];
    if (!tab || state.pendingCloses.includes(id)) continue;
    if ((change.status === null && id in statuses) || (change.status !== null && statuses[id] !== change.status)) {
      if (statuses === state.tabStatuses) statuses = { ...statuses };
      if (change.status === null) delete statuses[id];
      else statuses[id] = change.status;
    }
    if (!sameJson(tab.agent_state, change.agent_state)) {
      if (tabs === state.tabs) tabs = { ...tabs };
      tabs[id] = { ...tab, agent_state: change.agent_state };
    }
  }
  return { ...state, generation: delta.generation, tabs, tabStatuses: statuses };
}
