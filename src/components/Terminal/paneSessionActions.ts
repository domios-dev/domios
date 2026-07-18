import { invoke } from "../../lib/bridge";
import type { Pane } from "../../lib/types";
import { reattachPaneChat } from "./chatReattach";

export const PANE_SESSION_LABELS = {
  reconnect: "Reconnect",
  restart: "Restart session",
  copy: "Copy session ID",
} as const;

export interface PaneSessionActionView {
  sessionId: string | null;
  canReconnect: boolean;
  canRestart: boolean;
}

export function paneSessionActionView(pane: Pane): PaneSessionActionView {
  const chatBacking = pane.backing?.kind === "chat_backend" ? pane.backing : null;
  return {
    sessionId: pane.agent_session_id ?? chatBacking?.session_id ?? null,
    canReconnect: pane.agent_type != null || chatBacking !== null,
    canRestart: pane.agent_type != null && chatBacking === null,
  };
}

export async function reconnectPaneSession(paneId: string): Promise<void> {
  await reattachPaneChat(paneId);
}

export async function restartPaneSession(paneId: string): Promise<void> {
  await invoke("restart_agent_pane", { paneId });
}

export async function copyPaneSessionId(sessionId: string): Promise<void> {
  if (!navigator.clipboard) throw new Error("Clipboard is unavailable");
  await navigator.clipboard.writeText(sessionId);
}
