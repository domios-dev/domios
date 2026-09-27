export type TerminalHistorySource = "backend-buffer" | "transport" | "none";

export function terminalHistorySource(
  ptyId: string | null | undefined,
  isWeb: boolean,
  isRemote: boolean,
): TerminalHistorySource {
  if (!ptyId) return "none";
  if (isRemote || isWeb) return "transport";
  return "backend-buffer";
}
