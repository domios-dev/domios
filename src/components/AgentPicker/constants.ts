export const FIRST_RUN_GA_PANE_MARKER = "codeterm:first-run-general-agent";

export const PREFERRED_GENERAL_PROVIDER_ID = "claude";

export function isFirstRunGeneralPane(
  pane: { description?: string | null } | undefined,
): boolean {
  return pane?.description === FIRST_RUN_GA_PANE_MARKER;
}
