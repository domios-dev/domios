type Invoke = <T>(command: string, args: Record<string, unknown>) => Promise<T>;
type TabCloseResult = { confirmation_required?: boolean };

export async function requestTabClose(invoke: Invoke, workspaceId: string, tabId: string): Promise<boolean> {
  const target = { workspaceId, tabId };
  const result = await invoke<TabCloseResult>("mux_close_tab", target);
  if (!result?.confirmation_required) return true;
  if (!window.confirm("Close and keep the lane recoverable?")) return false;
  const confirmed = await invoke<TabCloseResult>("mux_close_tab", { ...target, force: true });
  if (confirmed?.confirmation_required) throw new Error("The daemon did not accept the close confirmation.");
  return true;
}
