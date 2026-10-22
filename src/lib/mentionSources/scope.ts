import type { Plan } from "../../components/Canvas/types";
import type { MentionScope } from "../composerMentions";

export function projectIdentity(path: string | null | undefined): string | null {
  const normalized = path?.replace(/\\/g, "/").replace(/\/+$/, "") || null;
  return normalized && (/^[a-z]:\//i.test(normalized) || normalized.startsWith("//")) ? normalized.toLowerCase() : normalized;
}

// Missing composer scope must never widen a query to the whole machine.
export function planInMentionScope(plan: Plan, scope: MentionScope): boolean {
  return !!scope.projectPath && projectIdentity(plan.project_path) === projectIdentity(scope.projectPath)
    && plan.status === "active" && plan.mode !== "complete"
    && (!scope.teamId || plan.team_id === scope.teamId);
}
