export type TerminalOrigin =
  | { kind: "tab"; id: string; label: string }
  | { kind: "closed"; id: string }
  | { kind: "unknown" };

/** Who started a team terminal, named by tab title; a closed creator keeps its id only as a tooltip. */
export function terminalOrigin(creatorId: string | null, creatorTitle: string | null | undefined, creatorOpen: boolean): TerminalOrigin {
  if (!creatorId) return { kind: "unknown" };
  if (!creatorOpen) return { kind: "closed", id: creatorId };
  return { kind: "tab", id: creatorId, label: creatorTitle?.trim() || "Untitled tab" };
}

export function cronReportTitle(targetTabId: string | null | undefined, targetTitle: string | null | undefined): string {
  if (!targetTabId) return "Runs in a private cron worker";
  const title = targetTitle?.trim();
  return title ? `Reports to ${title}` : "Reports to a closed tab";
}
