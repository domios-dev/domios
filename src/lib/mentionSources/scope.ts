import type { Plan } from "../../components/Canvas/types";
import type { MentionScope } from "../composerMentions";
import type { MultiplexerState } from "../types";

export function mentionScopeForTab(state: Pick<MultiplexerState, "tabs" | "workspaces">, tabId: string): MentionScope {
  const tab = state.tabs[tabId];
  const workspace = state.workspaces.find((item) => item.id === tab?.workspace_id || item.tabs.includes(tabId));
  return { tabId, workspaceId: workspace?.id ?? null, projectPath: workspace?.cwd ?? null, cwd: tab?.cwd ?? workspace?.cwd ?? null, teamId: tab?.team_id ?? null };
}

export function projectIdentity(path: string | null | undefined): string | null {
  const normalized = path?.replace(/\\/g, "/").replace(/^\/\/\?\/UNC\//i, "//").replace(/^\/\/\?\//, "").replace(/\/+$/, "") || null;
  return normalized && (/^[a-z]:\//i.test(normalized) || normalized.startsWith("//")) ? normalized.toLowerCase() : normalized;
}

// Missing composer scope must never widen a query to the whole machine.
export function planInMentionScope(plan: Plan, scope: MentionScope): boolean {
  return !!scope.projectPath && projectIdentity(plan.project_path) === projectIdentity(scope.projectPath)
    && plan.status === "active" && plan.mode !== "complete"
    && (plan.team_id ?? null) === scope.teamId;
}
