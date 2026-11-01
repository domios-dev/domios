import type { Tab } from "../../lib/types";
import type { ExtendedState } from "../../store/multiplexer/tree";

const sidebarFields = [
  "tabs", "workspaces", "teams", "active_workspace_id", "activeTabIds",
  "notifications", "tabStatuses",
] as const;

export type SidebarState = Pick<ExtendedState, typeof sidebarFields[number]>;

export function selectSidebarState(state: ExtendedState): SidebarState {
  return Object.fromEntries(sidebarFields.map((key) => [key, state[key]])) as SidebarState;
}

function equalValue(a: unknown, b: unknown, ...ignoredKeys: string[]): boolean {
  if (Object.is(a, b)) return true;
  if (!a || !b || typeof a !== "object" || typeof b !== "object") return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const left = a as Record<string, unknown>;
  const right = b as Record<string, unknown>;
  const keys = new Set([...Object.keys(left), ...Object.keys(right)]);
  for (const key of keys) {
    if (ignoredKeys.includes(key)) continue;
    if (!equalValue(left[key], right[key])) return false;
  }
  return true;
}

// A working tab's streamed line, phase and timer reach only its own row, which reads them live.
const ROW_LIVE_FIELDS = ["status_line", "phase", "elapsed_ms", "last_turn_summary"];

function sameSidebarAgentState(a: Tab["agent_state"], b: Tab["agent_state"]): boolean {
  if (a?.state === "working" && b?.state === "working") return equalValue(a, b, ...ROW_LIVE_FIELDS);
  return equalValue(a, b, "elapsed_ms");
}

export function sidebarStateEqual(a: SidebarState, b: SidebarState): boolean {
  for (const field of sidebarFields) {
    if (field !== "tabs") {
      if (!equalValue(a[field], b[field])) return false;
      continue;
    }
    const ids = new Set([...Object.keys(a.tabs), ...Object.keys(b.tabs)]);
    for (const id of ids) {
      const left = a.tabs[id];
      const right = b.tabs[id];
      if (left === right) continue;
      if (!left || !right) return false;
      if (!equalValue(left, right, "agent_state", "last_active_at_ms")) return false;
      if (!sameSidebarAgentState(left.agent_state, right.agent_state)) return false;
    }
  }
  return true;
}
