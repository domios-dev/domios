/** Copy for the new-session picker (task_234c14f0) — one source so the
 *  destination preview line can never drift from what confirm actually does. */

export const WORKSPACE_HELP_TEXT =
  "A workspace groups related tabs — usually one project or effort. Your new session opens as a tab inside it.";

export const MODE_HELP_TEXT =
  "Single agent starts one chat. Orchestration pairs a lead agent with a Planner peer that drafts the plan with you.";

export const NEW_WORKSPACE_OPTION_VALUE = "__new_workspace__";

export interface DestinationPreviewInput {
  mode: "single" | "pair";
  workspaceName: string;
  singleProviderLabel?: string;
  singleModelLabel?: string;
}

/** The exact line shown under the form — derived from the SAME values the
 *  confirm handler spawns with, never a separately-maintained copy. */
export function buildDestinationPreview({
  mode,
  workspaceName,
  singleProviderLabel,
  singleModelLabel,
}: DestinationPreviewInput): string {
  if (mode === "pair") {
    return `Orchestrator + Planner will open as one group in ${workspaceName}`;
  }
  const agent = singleModelLabel
    ? `${singleProviderLabel ?? "Agent"} (${singleModelLabel})`
    : (singleProviderLabel ?? "Agent");
  return `${agent} will open in ${workspaceName}`;
}
