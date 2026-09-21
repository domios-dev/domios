import { invoke } from "./bridge";
import { uid } from "./uid";
import { IS_WEB } from "./webApi";

const CLIENT_ID_KEY = "codeterm.client_id.v1";

export interface ResizeOwner {
  tab_id: string;
  pty_id: string;
  client_id: string;
  cols: number;
  rows: number;
  updated_at: number;
}

export interface ResizeOwnerClaim {
  owner: ResizeOwner;
  current_size: { cols: number; rows: number } | null;
}

export function resizeOwnerChangedEvent(tabId?: string | null): string {
  return tabId?.startsWith("mesh:")
    ? `pty-resize-owner-changed:${tabId}`
    : "pty-resize-owner-changed";
}

export function getResizeClientId(): string {
  const prefix = IS_WEB ? "web:" : "tauri:";
  if (typeof window === "undefined") return "server";
  try {
    const existing = window.localStorage.getItem(CLIENT_ID_KEY);
    if (existing?.startsWith(prefix)) return existing;
    const next = prefix + uid();
    window.localStorage.setItem(CLIENT_ID_KEY, next);
    return next;
  } catch {
    return prefix + "ephemeral-" + Math.random().toString(36).slice(2);
  }
}

export async function claimResizeOwner(
  tabId: string,
  ptyId: string,
  clientId: string,
  cols: number,
  rows: number,
  force = false,
): Promise<ResizeOwnerClaim> {
  return invoke<ResizeOwnerClaim>("claim_resize_owner", {
    tabId,
    ptyId,
    clientId,
    cols,
    rows,
    force,
  });
}
