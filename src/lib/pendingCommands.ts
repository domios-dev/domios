// Pending commands and cwd overrides for newly created tabs.

const pendingCmds = new Map<string, string>();
const pendingCwds = new Map<string, string>();

export function setPendingCommand(tabId: string, command: string): void {
  pendingCmds.set(tabId, command);
}

export function consumePendingCommand(tabId: string): string | undefined {
  const cmd = pendingCmds.get(tabId);
  if (cmd) pendingCmds.delete(tabId);
  return cmd;
}

export function setPendingCwd(tabId: string, cwd: string): void {
  pendingCwds.set(tabId, cwd);
}

export function consumePendingCwd(tabId: string): string | undefined {
  const cwd = pendingCwds.get(tabId);
  if (cwd) pendingCwds.delete(tabId);
  return cwd;
}
