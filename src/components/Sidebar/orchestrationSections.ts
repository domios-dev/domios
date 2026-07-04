import type { Pane } from "../../lib/types";

/** The two sibling sections inside an orchestration group. Managers are the
 *  Orchestrator's delegated leadership (Planner today; watcher roles later);
 *  Workers are the execution leaves. The Orchestrator itself is the group
 *  head — never a section member. Single source of truth for the sidebar, the
 *  group-close confirm dialog, and any future surface. */
export type OrchestrationSection = "manager" | "worker";

export const SECTION_LABEL: Record<OrchestrationSection, string> = {
  manager: "Managers",
  worker: "Workers",
};

/** role_profile values that render under Managers. Extend this set when a new
 *  leadership specialization (e.g. a watcher) lands — every consumer follows. */
export const MANAGER_ROLE_PROFILES: ReadonlySet<string> = new Set(["planner", "watcher"]);

/** Which section a child pane belongs to. Keys off the specialization
 *  (`role_profile`), since the structural AgentRole is only Orchestrator|Worker.
 *  Unknown specializations fall to Workers with a debug breadcrumb so a new
 *  leadership role isn't silently mis-bucketed — never throws. */
export function sectionForRoleProfile(
  roleProfile: string | null | undefined,
): OrchestrationSection {
  const key = roleProfile?.trim().toLowerCase() ?? "";
  if (MANAGER_ROLE_PROFILES.has(key)) return "manager";
  if (key !== "" && key !== "worker") {
    console.debug(`[orchestrationSections] unknown role_profile "${roleProfile}" → Workers`);
  }
  return "worker";
}

/** Role-badge label shown in place of the model chip for group leadership:
 *  ORCHESTRATOR for the head, the uppercased specialization for a Manager
 *  (PLANNER, WATCHER, …), null for a Worker (which keeps its model chip). */
export function roleBadgeLabel(
  pane: Pick<Pane, "role" | "role_profile"> | null | undefined,
): string | null {
  if (!pane) return null;
  if (pane.role === "Orchestrator") return "ORCHESTRATOR";
  if (sectionForRoleProfile(pane.role_profile) === "manager") {
    return (pane.role_profile ?? "").trim().toUpperCase();
  }
  return null;
}

/** testid suffix for a role badge — role-badge-orchestrator, role-badge-planner. */
export function roleBadgeTestId(label: string): string {
  return `role-badge-${label.toLowerCase()}`;
}

export interface SectionPartition<T> {
  managers: T[];
  workers: T[];
}

/** Split a group's child nodes into Managers/Workers, preserving incoming
 *  (spawn) order within each section. */
export function partitionBySection<T extends { pane: Pick<Pane, "role_profile"> }>(
  nodes: readonly T[],
): SectionPartition<T> {
  const managers: T[] = [];
  const workers: T[] = [];
  for (const node of nodes) {
    if (sectionForRoleProfile(node.pane.role_profile) === "manager") managers.push(node);
    else workers.push(node);
  }
  return { managers, workers };
}

export interface GroupSectionCounts {
  managers: number;
  workers: number;
}

export function countBySection(
  members: readonly { roleProfile?: string | null }[],
): GroupSectionCounts {
  let managers = 0;
  let workers = 0;
  for (const m of members) {
    if (sectionForRoleProfile(m.roleProfile) === "manager") managers += 1;
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

/** A section header earns its row ONLY when it disambiguates: either both
 *  sections are present (so the reader needs to tell them apart) or this section
 *  has more than one member (so the label groups a set). The common one-planner
 *  group therefore drops the MANAGERS row entirely — the containment line and
 *  the single indented row already say everything. */
export function shouldShowSectionHeader(
  section: OrchestrationSection,
  counts: GroupSectionCounts,
): boolean {
  const bothPresent = counts.managers > 0 && counts.workers > 0;
  const self = section === "manager" ? counts.managers : counts.workers;
  return bothPresent || self > 1;
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
