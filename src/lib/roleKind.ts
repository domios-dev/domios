import type { AgentRole, Tab } from "./types";

/** Built-in role kinds. The kind lives in `role_profile`; `role` is structural. */
export const ORCHESTRATOR_KIND = "orchestrator";
export const GENERAL_AGENT_KIND = "general_agent";
export const PLANNER_KIND = "planner";
export const WATCHER_KIND = "watcher";
export const WORKER_KIND = "worker";
export const CURATOR_KIND = "curator";
export const CARTOGRAPHER_KIND = "cartographer";
export const QA_KIND = "qa";
export const RELEASE_MANAGER_KIND = "release_manager";
export const REVIEWER_KIND = "reviewer";

/** Kinds that anchor a Team — the only ones that take no parent and spawn. */
const ANCHOR_KINDS = new Set([ORCHESTRATOR_KIND, GENERAL_AGENT_KIND]);
/** Manager-kind roles. A Worker naming one collapses to the plain Worker. */
const MANAGING_KINDS = new Set([
  ...ANCHOR_KINDS,
  PLANNER_KIND,
  WATCHER_KIND,
  CARTOGRAPHER_KIND,
  CURATOR_KIND,
]);
/** Kinds that may manage the Team they belong to (anchors plus managing leaves). */
const TEAM_AUTHORITY_KINDS = new Set([...ANCHOR_KINDS, PLANNER_KIND, WATCHER_KIND]);

const KIND_LABELS: Record<string, string> = {
  [ORCHESTRATOR_KIND]: "Orchestrator",
  [GENERAL_AGENT_KIND]: "General Agent",
  [PLANNER_KIND]: "Planner",
  [WATCHER_KIND]: "Watcher",
  [WORKER_KIND]: "Worker",
  [CARTOGRAPHER_KIND]: "Cartographer",
  [CURATOR_KIND]: "Curator",
  [QA_KIND]: "QA",
  [RELEASE_MANAGER_KIND]: "Release manager",
  [REVIEWER_KIND]: "Reviewer",
};

type RoleCarrier = Pick<Tab, "role" | "role_profile"> & { role_label?: string | null };

/**
 * Resolve a tab's role kind. A Worker's profile is a subtype and can never
 * borrow a managing kind, mirroring the native `role_kind::kind_id`.
 */
export function roleKind(tab: RoleCarrier | null | undefined): string | null {
  if (!tab?.role) return null;
  const profile = tab.role_profile?.trim() || null;
  if (tab.role === "Worker") {
    return profile && !MANAGING_KINDS.has(profile) ? profile : WORKER_KIND;
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

/** Display label for a tab: its Team-registered label, then its kind, then the structural role. */
export function roleKindLabel(tab: RoleCarrier | null | undefined): string | null {
  const kind = roleKind(tab);
  if (kind === null) return null;
  return tab?.role_label?.trim() || KIND_LABELS[kind] || structuralLabel(tab?.role ?? null);
}

/** The host name of a built-in kind, such as a manager slot; any other key reads in sentence case. */
export function kindDisplayName(kind: string): string {
  return KIND_LABELS[kind] ?? humanizeRoleKey(kind);
}

const ROLE_ACRONYMS = new Set(["qa", "ci", "ui", "ux", "api", "pr", "sre"]);

/** A role or file key in sentence case: "build_owner" → "Build owner", "qa-lead" → "QA lead". */
export function humanizeRoleKey(key: string): string {
  const words = key.trim().split(/[_\-\s.]+/).filter(Boolean);
  if (words.length === 0) return key.trim();
  return words.map((word, index) => {
    const lower = word.toLowerCase();
    if (ROLE_ACRONYMS.has(lower)) return lower.toUpperCase();
    return index === 0 ? `${lower.charAt(0).toUpperCase()}${lower.slice(1)}` : lower;
  }).join(" ");
}

const KEY_LIKE = /^[a-z0-9]+([_-][a-z0-9]+)*$/;
const normalizedKey = (text: string) => text.toLowerCase().replace(/[_\-\s]+/g, " ").trim();

/**
 * A role's label for people: a declared display name ("Lead", "Conductor") stays as written; a label that only
 * restates its key, or is itself a key, reads as the kind's host name or in sentence case ("Field scout").
 */
export function roleDisplayLabel(key: string | null | undefined, label?: string | null): string {
  const declared = label?.trim() ?? "";
  const id = key?.trim() ?? "";
  if (declared && !KEY_LIKE.test(declared) && normalizedKey(declared) !== normalizedKey(id)) return declared;
  const source = id || declared;
  return KIND_LABELS[source] ?? humanizeRoleKey(declared || source);
}

function structuralLabel(role: AgentRole | null): string | null {
  return role;
}
