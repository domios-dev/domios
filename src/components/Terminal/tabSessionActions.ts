import { invoke } from "../../lib/bridge";
import type { Tab } from "../../lib/types";
import { reattachTabChat } from "./chatReattach";

export const TAB_SESSION_LABELS = {
  reconnect: "Reconnect",
  restart: "Restart session",
  copy: "Copy session ID",
} as const;

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
