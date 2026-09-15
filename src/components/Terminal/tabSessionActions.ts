import { invoke } from "../../lib/bridge";
import type { ProviderDescriptor, Tab } from "../../lib/types";
import { reattachTabChat } from "./chatReattach";

export const TAB_SESSION_LABELS = {
  reconnect: "Reconnect",
  reconnecting: "Reconnecting…",
  restart: "Restart session",
  restarting: "Restarting…",
  copy: "Copy session ID",
  autoConfirm: "Auto-confirm permissions",
  autoConfirmHint:
    "Answers Yes once to tool permission prompts the provider recognizes. Questions, trust dialogs and anything unrecognized stay for you.",
} as const;

/** The tab's persisted opt-in, or `null` when its provider cannot recognize permission prompts. */
export function permissionAutoConfirmMenuState(
  tab: Tab,
  descriptor: ProviderDescriptor | null | undefined,
): boolean | null {
  if (tab.backing?.kind === "chat_backend" || tab.agent_type == null) return null;
  if (descriptor?.supports_permission_auto_confirm !== true) return null;
  return tab.permission_auto_confirm === true;
}

export async function setTabPermissionAutoConfirm(tabId: string, enabled: boolean): Promise<void> {
  await invoke("mux_set_permission_auto_confirm", { tabId, enabled });
}

export interface TabSessionActionView {
  sessionId: string | null;
  canReconnect: boolean;
  canRestart: boolean;
}

export function tabSessionActionView(tab: Tab): TabSessionActionView {
  const chatBacking = tab.backing?.kind === "chat_backend" ? tab.backing : null;
  return {
    sessionId: tab.agent_session_id ?? chatBacking?.session_id ?? null,
    canReconnect: tab.agent_type != null || chatBacking !== null,
    canRestart: tab.agent_type != null && chatBacking === null,
  };
}

export async function reconnectTabSession(tabId: string): Promise<void> {
  await reattachTabChat(tabId);
}

export async function restartTabSession(tabId: string): Promise<void> {
  await invoke("restart_agent_tab", { tabId });
}

export async function copyTabSessionId(sessionId: string): Promise<void> {
  if (!navigator.clipboard) throw new Error("Clipboard is unavailable");
  await navigator.clipboard.writeText(sessionId);
}
