import { invoke } from "./bridge";

const CLIENT_ID_KEY = "codeterm.client_id.v1";

export interface ResizeOwner {
  pane_id: string;
  pty_id: string;
  client_id: string;
  cols: number;
  rows: number;
  updated_at: number;
}

export function getResizeClientId(): string {
  if (typeof window === "undefined") return "server";
  try {
    const existing = window.localStorage.getItem(CLIENT_ID_KEY);
    if (existing) return existing;
    const next = crypto.randomUUID();
    window.localStorage.setItem(CLIENT_ID_KEY, next);
    return next;
  } catch {
    return "ephemeral-" + Math.random().toString(36).slice(2);
  }
}

export async function claimResizeOwner(
  paneId: string,
  ptyId: string,
  clientId: string,
  cols: number,
  rows: number,
): Promise<ResizeOwner> {
  return invoke<ResizeOwner>("claim_resize_owner", {
    paneId,
    ptyId,
    clientId,
    cols,
    rows,
  });
}
