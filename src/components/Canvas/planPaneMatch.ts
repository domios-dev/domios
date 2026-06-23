import type { Pane } from "../../lib/types";
import type { Plan } from "./types";

export function collectPaneChain(
  paneId: string,
  panes: Record<string, Pane>,
): Pane[] {
  const chain: Pane[] = [];
  const seen = new Set<string>();
  let cur: string | null | undefined = paneId;
  while (cur && !seen.has(cur)) {
    seen.add(cur);
    const pane: Pane | undefined = panes[cur];
    if (!pane) break;
    chain.push(pane);
    cur = pane.parent_pane_id;
  }
  return chain;
}

export function resolveOrchestratorPaneId(
  paneId: string,
  panes: Record<string, Pane>,
): string {
  for (const pane of collectPaneChain(paneId, panes)) {
    if (pane.role === "Orchestrator" || pane.can_spawn) return pane.id;
  }
  return paneId;
}

function planMatchesPaneChain(plan: Plan, chain: Pane[]): boolean {
  const paneIds = new Set(chain.map((p) => p.id));
  const sessionIds = new Set(
    chain.map((p) => p.agent_session_id).filter((s): s is string => !!s),
  );
  if (plan.owner.pane_id && paneIds.has(plan.owner.pane_id)) return true;
  if (plan.owner.session_id && sessionIds.has(plan.owner.session_id)) return true;
  return false;
}

export function findPlansForPane(
  plans: Plan[],
  paneId: string,
  panes: Record<string, Pane>,
): Plan[] {
  const active = plans.filter((p) => p.status === "active");
  const chain = collectPaneChain(paneId, panes);
  return active.filter((plan) => {
    if (plan.tasks.some((t) => t.assigned_pane_id === paneId)) return true;
    if (plan.owner.pane_id === paneId) return true;
    return planMatchesPaneChain(plan, chain);
  });
}

export function filterPlansForCanvas(
  plans: Plan[],
  mode: "workspace" | "focus",
  opts: { planId?: string; orchestratorPaneId?: string; panes?: Record<string, Pane> },
): Plan[] {
  const active = plans.filter((p) => p.status === "active");
  if (mode === "workspace") return active;
  if (opts.planId) return active.filter((p) => p.id === opts.planId);
  if (opts.orchestratorPaneId && opts.panes) {
    const chain = collectPaneChain(opts.orchestratorPaneId, opts.panes);
    return active.filter((p) => planMatchesPaneChain(p, chain));
  }
  return active;
}

export function findSectionInPlan(plan: Plan, sectionId: string) {
  function walk(sections: Plan["sections"]): Plan["sections"][number] | null {
    for (const s of sections) {
      if (s.id === sectionId) return s;
      const sub = walk(s.subsections);
      if (sub) return sub;
    }
    return null;
  }
  return walk(plan.sections);
}
