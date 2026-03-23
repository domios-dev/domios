// Pending commands and cwd overrides for newly created panes.

const pendingCmds = new Map<string, string>();
const pendingCwds = new Map<string, string>();

export function setPendingCommand(paneId: string, command: string): void {
  pendingCmds.set(paneId, command);
}

export function consumePendingCommand(paneId: string): string | undefined {
  const cmd = pendingCmds.get(paneId);
  if (cmd) pendingCmds.delete(paneId);
  return cmd;
}

export function setPendingCwd(paneId: string, cwd: string): void {
  pendingCwds.set(paneId, cwd);
}

export function consumePendingCwd(paneId: string): string | undefined {
  const cwd = pendingCwds.get(paneId);
  if (cwd) pendingCwds.delete(paneId);
  return cwd;
}
