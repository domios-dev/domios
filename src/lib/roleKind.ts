import type { AgentRole, Tab } from "./types";

/** Built-in role kinds. The kind lives in `role_profile`; `role` is structural. */
export const ORCHESTRATOR_KIND = "orchestrator";
export const GENERAL_AGENT_KIND = "general_agent";
export const PLANNER_KIND = "planner";
export const WATCHER_KIND = "watcher";
export const WORKER_KIND = "worker";

/** Kinds that anchor a Team — the only ones that take no parent and spawn. */
const ANCHOR_KINDS = new Set([ORCHESTRATOR_KIND, GENERAL_AGENT_KIND]);
/** Kinds that may manage the Team they belong to (anchors plus managing leaves). */
const TEAM_AUTHORITY_KINDS = new Set([...ANCHOR_KINDS, PLANNER_KIND, WATCHER_KIND]);

const KIND_LABELS: Record<string, string> = {
  [ORCHESTRATOR_KIND]: "Orchestrator",
  [GENERAL_AGENT_KIND]: "General Agent",
  [PLANNER_KIND]: "Planner",
  [WATCHER_KIND]: "Watcher",
  [WORKER_KIND]: "Worker",
};

type RoleCarrier = Pick<Tab, "role" | "role_profile">;

/**
 * Resolve a tab's role kind. A Worker's profile is a subtype and can never
 * borrow a managing kind, mirroring the native `role_kind::kind_id`.
 */
export function roleKind(tab: RoleCarrier | null | undefined): string | null {
  if (!tab?.role) return null;
  const profile = tab.role_profile?.trim() || null;
  if (tab.role === "Worker") {
    return profile && !TEAM_AUTHORITY_KINDS.has(profile) ? profile : WORKER_KIND;
  }
  return profile;
}

/** True when the tab anchors its own Team (Orchestrator or General Agent). */
export function anchorsTeam(tab: RoleCarrier | null | undefined): boolean {
  const kind = roleKind(tab);
  return kind !== null && ANCHOR_KINDS.has(kind);
}

/** True when the tab may rename, adopt into, or close the Team it belongs to. */
export function hasTeamAuthority(tab: RoleCarrier | null | undefined): boolean {
  const kind = roleKind(tab);
  return kind !== null && TEAM_AUTHORITY_KINDS.has(kind);
}

export function isPlanner(tab: RoleCarrier | null | undefined): boolean {
  return roleKind(tab) === PLANNER_KIND;
}

export function isGeneralAgent(tab: RoleCarrier | null | undefined): boolean {
  return roleKind(tab) === GENERAL_AGENT_KIND;
}

/** Display label for a tab's kind, falling back to the structural role. */
export function roleKindLabel(tab: RoleCarrier | null | undefined): string | null {
  const kind = roleKind(tab);
  if (kind === null) return null;
  return KIND_LABELS[kind] ?? structuralLabel(tab?.role ?? null);
}

function structuralLabel(role: AgentRole | null): string | null {
  return role;
}
