import type { Tab } from "../../lib/types";

/** The two sibling sections inside an orchestration group. Managers are the
 *  Orchestrator's delegated leadership (Planner, Watcher);
 *  Workers are the execution leaves. The Orchestrator itself is the group
 *  head — never a section member. Single source of truth for the sidebar, the
 *  group-close confirm dialog, and any future surface. */
export type OrchestrationSection = "manager" | "worker";

export const SECTION_LABEL: Record<OrchestrationSection, string> = {
  manager: "Managers",
  worker: "Workers",
};

export function sectionForRole(
  role: string | null | undefined,
  roleProfile?: string | null,
): OrchestrationSection {
  if (role === "Manager") return "manager";
  const key = roleProfile?.trim().toLowerCase() ?? "";
  if (key !== "" && key !== "worker" && role === "Worker") {
    console.debug(`[orchestrationSections] worker role_profile "${roleProfile}" ignored`);
  }
  return "worker";
}

/** Leadership label for the model-chip slot (legacy surfaces). */
export function roleBadgeLabel(
  tab: Pick<Tab, "role" | "role_profile"> | null | undefined,
): string | null {
  if (!tab) return null;
  const kind = roleKind(tab);
  return tab.role === "Manager" && kind ? kind.toUpperCase() : null;
}

/** Role shield on tab line 2 — leadership only: ORCHESTRATOR / PLANNER / WATCHER. */
export function roleShieldLabel(
  tab: Pick<Tab, "role" | "role_profile"> | null | undefined,
): string | null {
  if (!tab?.role) return null;
  const kind = roleKind(tab);
  return tab.role === "Manager" && kind ? kind.toUpperCase() : null;
}

/** testid suffix for a role badge — role-badge-orchestrator, role-badge-planner. */
export function roleBadgeTestId(label: string): string {
  return `role-badge-${label.toLowerCase()}`;
}

export interface SectionPartition<T> {
  managers: T[];
  workers: T[];
}

function managerRank(role: string | null | undefined): number {
  return role === "Orchestrator" ? 0 : 1;
}

export function sortTeamManagers<T extends { tab: Pick<Tab, "role" | "role_profile"> }>(
  members: readonly T[],
): T[] {
  return [...members].sort((a, b) => managerRank(a.tab) - managerRank(b.tab));
}

/** Split a group's child nodes into Managers/Workers, preserving incoming
 *  (spawn) order within each section. */
export function partitionBySection<T extends { tab: Pick<Tab, "role" | "role_profile"> }>(
  nodes: readonly T[],
): SectionPartition<T> {
  const managers: T[] = [];
  const workers: T[] = [];
  for (const node of nodes) {
    if (sectionForRole(node.tab.role, node.tab.role_profile) === "manager") managers.push(node);
    else workers.push(node);
  }
  return { managers, workers };
}

export interface GroupSectionCounts {
  managers: number;
  workers: number;
}

export function countBySection(
  members: readonly { role?: string | null; roleProfile?: string | null }[],
): GroupSectionCounts {
  let managers = 0;
  let workers = 0;
  for (const m of members) {
    if (sectionForRole(m.role, m.roleProfile) === "manager") managers += 1;
    else workers += 1;
  }
  return { managers, workers };
}

function plural(n: number, singular: string): string {
  return `${n} ${singular}${n === 1 ? "" : "s"}`;
}

/** Human summary for the group-close dialog: "1 manager, 3 workers". Omits a
 *  zero section so a worker-only group reads "3 workers", not "0 managers, …". */
export function summarizeGroupClose(counts: GroupSectionCounts): string {
  const parts: string[] = [];
  if (counts.managers > 0) parts.push(plural(counts.managers, "manager"));
  if (counts.workers > 0) parts.push(plural(counts.workers, "worker"));
  return parts.join(", ");
}

/** A section header renders whenever its section is non-empty — even a lone
 *  manager gets its MANAGERS separator so the group structure is always
 *  explicit. Empty sections never render a header. */
export function shouldShowSectionHeader(
  section: OrchestrationSection,
  counts: GroupSectionCounts,
): boolean {
  const self = section === "manager" ? counts.managers : counts.workers;
  return self > 0;
}

/** Collapsed group-head summary: "1 manager · 2 workers". Same count source as
 *  the close dialog, mid-dot separated so it reads as one compact glance on the
 *  orchestrator row rather than a sentence. Never renders a zero section. */
export function summarizeGroupCounts(counts: GroupSectionCounts): string {
  const parts: string[] = [];
  if (counts.managers > 0) parts.push(plural(counts.managers, "manager"));
  if (counts.workers > 0) parts.push(plural(counts.workers, "worker"));
  return parts.join(" · ");
}

export function summarizeWorkers(total: number, working: number): string {
  return `${plural(total, "worker")} · ${working} working`;
}
