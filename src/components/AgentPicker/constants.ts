export const FIRST_RUN_GA_TAB_MARKER = "codeterm:first-run-general-agent";

export const PREFERRED_GENERAL_PROVIDER_ID = "claude";

export function isFirstRunGeneralTab(
  tab: { description?: string | null } | undefined,
): boolean {
  return tab?.description === FIRST_RUN_GA_TAB_MARKER;
}
