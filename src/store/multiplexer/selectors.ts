import type { Notification, Tab, Team } from "../../lib/types";
import type { ExtendedState } from "./tree";

const TITLE_SPINNER_PREFIX = /^[✻✳✴✵✶✷✸✹✺✻✼✽✾✿❀❁❂❃❇❈❉❊❋*]\s*/u;

export function stripTabTitleSpinner(title: string | undefined): string | undefined {
  return title?.replace(TITLE_SPINNER_PREFIX, "").trim();
}

export function selectTab(state: ExtendedState, tabId: string): Tab | undefined {
  return state.tabs[tabId];
}

export function selectTeams(state: ExtendedState): Team[] {
  return state.teams;
}

export function teamsEqual(a: Team[], b: Team[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  return a.every((team, index) => {
    const other = b[index];
    return team.id === other.id
      && team.name === other.name
      && team.lifecycle === other.lifecycle
      && team.created_at === other.created_at
      && team.workspace_id === other.workspace_id;
  });
}

export function tabFieldsEqual(a: Tab | undefined, b: Tab | undefined): boolean {
  if (a === b) return true;
  if (!a || !b) return !a && !b;
  const keys = new Set([...Object.keys(a), ...Object.keys(b)] as (keyof Tab)[]);
  for (const key of keys) {
    const av = a[key];
    const bv = b[key];
    if (av === bv) continue;
    if (key === "agent_state") {
      if (!shallowRecordEqual(av as Record<string, unknown> | null | undefined, bv as Record<string, unknown> | null | undefined)) {
        return false;
      }
      continue;
    }
    if (key === "pending_prompt") {
      if (!shallowRecordEqual(av as Record<string, unknown> | null | undefined, bv as Record<string, unknown> | null | undefined)) {
        return false;
      }
      continue;
    }
    if (key === "backing") {
      if (!shallowRecordEqual(av as Record<string, unknown> | null | undefined, bv as Record<string, unknown> | null | undefined)) {
        return false;
      }
      continue;
    }
    if (key === "watcher") {
      if (!shallowRecordEqual(av as Record<string, unknown> | null | undefined, bv as Record<string, unknown> | null | undefined)) {
        return false;
      }
      continue;
    }
    return false;
  }
  return true;
}

function shallowRecordEqual(
  a: Record<string, unknown> | null | undefined,
  b: Record<string, unknown> | null | undefined,
): boolean {
  if (a === b) return true;
  if (!a || !b) return !a && !b;
  const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
  for (const key of keys) {
    if (a[key] !== b[key]) return false;
  }
  return true;
}

export function selectTabTitleMap(state: ExtendedState): Map<string, string> {
  const byShort = new Map<string, string>();
  for (const ws of state.workspaces) {
    for (const tabId of ws.tabs) {
      const tab = state.tabs[tabId];
      const name = stripTabTitleSpinner(tab?.title);
      if (name) byShort.set(tabId.slice(0, 8), name);
    }
  }
  return byShort;
}

export function tabTitleMapEqual(a: Map<string, string>, b: Map<string, string>): boolean {
  if (a === b) return true;
  if (a.size !== b.size) return false;
  for (const [key, value] of a) {
    if (b.get(key) !== value) return false;
  }
  return true;
}

export function selectTabAgentTypes(state: ExtendedState): Record<string, string | null | undefined> {
  const out: Record<string, string | null | undefined> = {};
  for (const [id, tab] of Object.entries(state.tabs)) {
    out[id] = tab.agent_type;
  }
  return out;
}

export function tabAgentTypesEqual(
  a: Record<string, string | null | undefined>,
  b: Record<string, string | null | undefined>,
): boolean {
  if (a === b) return true;
  const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
  for (const key of keys) {
    if (a[key] !== b[key]) return false;
  }
  return true;
}

export interface PlanNavContext {
  workspaces: ExtendedState["workspaces"];
  activeWorkspaceId: ExtendedState["active_workspace_id"];
}

export function selectPlanNavContext(state: ExtendedState): PlanNavContext {
  return {
    workspaces: state.workspaces,
    activeWorkspaceId: state.active_workspace_id,
  };
}

export function tabsRecordEqual(
  a: Record<string, Tab>,
  b: Record<string, Tab>,
): boolean {
  if (a === b) return true;
  const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
  for (const key of keys) {
    if (!tabFieldsEqual(a[key], b[key])) return false;
  }
  return true;
}

export function planNavContextEqual(a: PlanNavContext, b: PlanNavContext): boolean {
  if (a.activeWorkspaceId !== b.activeWorkspaceId) return false;
  if (a.workspaces.length !== b.workspaces.length) return false;
  for (let i = 0; i < a.workspaces.length; i += 1) {
    const aw = a.workspaces[i];
    const bw = b.workspaces[i];
    if (aw.id !== bw.id || aw.active_tab_id !== bw.active_tab_id || aw.tabs.length !== bw.tabs.length) {
      return false;
    }
    for (let j = 0; j < aw.tabs.length; j += 1) {
      if (aw.tabs[j] !== bw.tabs[j]) {
        return false;
      }
    }
  }
  return true;
}

/** The questions this tab asked. The transcript card and the bell panel both
 *  read these records, so neither surface holds an answer of its own. */
export function selectTabAsks(state: ExtendedState, tabId: string): Notification[] {
  return state.notifications.filter((notification) => notification.answer?.asked_from_tab === tabId);
}

export function tabAsksEqual(a: Notification[], b: Notification[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  return a.every((ask, index) => {
    const other = b[index];
    return ask.id === other.id
      && ask.timestamp === other.timestamp
      && ask.title === other.title
      && ask.body === other.body
      && ask.severity === other.severity
      && ask.actions === other.actions
      && ask.answer?.answered === other.answer?.answered
      && ask.answer?.asked_from_tab === other.answer?.asked_from_tab;
  });
}
