import { invoke } from "./bridge";
import { uid } from "./uid";

const CLIENT_ID_KEY = "codeterm.client_id.v1";

export type ResizeOwner = import("../../gen/ResizeOwner").ResizeOwner;

export type ResizeOwnerClaim = import("../../gen/ResizeOwnerClaim").ResizeOwnerClaim;

export function resizeOwnerChangedEvent(tabId?: string | null): string {
  return tabId?.startsWith("mesh:")
    ? `pty-resize-owner-changed:${tabId}`
    : "pty-resize-owner-changed";
}

/**
 * One id per window: two windows of one browser are two views of a PTY and
 * must not both believe they own its size. Session storage survives a reload
 * of the same window, so ownership survives a reconnect.
 */
export function getResizeClientId(): string {
  const prefix = "client:";
  if (typeof window === "undefined") return "server";
  try {
    const existing = window.sessionStorage.getItem(CLIENT_ID_KEY);
    if (existing?.startsWith(prefix)) return existing;
    const next = prefix + uid();
    window.sessionStorage.setItem(CLIENT_ID_KEY, next);
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
