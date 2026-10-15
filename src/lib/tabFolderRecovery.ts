export async function recoverTabFolder(tabId: string, cwd: string, ports: {
  update: (tabId: string, cwd: string) => Promise<unknown>;
  restart: (tabId: string, fresh: false) => Promise<unknown>;
}): Promise<void> {
  await ports.update(tabId, cwd);
  await ports.restart(tabId, false);
}
