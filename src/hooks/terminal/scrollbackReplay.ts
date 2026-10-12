export type TerminalHistorySource = "transport" | "none";

export function terminalHistorySource(
  ptyId: string | null | undefined,
  _isWeb: boolean,
  _isRemote: boolean,
): TerminalHistorySource {
  return ptyId ? "transport" : "none";
}
