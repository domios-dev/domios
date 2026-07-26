import type { PaneUiMode } from "./chatUiMode";

const PREFIX = "codeterm:pane-ui-mode:";
// default, remote-control) — TerminalPane records those as user overrides.
const EVENT = "codeterm-client-pane-ui-mode";
const EVENT_CURRENT = "codeterm-client-pane-ui-mode-current";
const currentModes = new Map<string, PaneUiMode>();

export function getCurrentPaneUiMode(paneId: string): PaneUiMode | null {
  return currentModes.get(paneId) ?? null;
}

export function publishCurrentPaneUiMode(paneId: string, mode: PaneUiMode): void {
  currentModes.set(paneId, mode);
  window.dispatchEvent(
    new CustomEvent(EVENT_CURRENT, {
      detail: { paneId, mode },
    }),
  );
}

/** Single source of truth for a pane's opening view, shared by the pane body and
 *  the mode switch so they agree on the first frame (no terminal→chat flicker).
 *  Priority: explicit persisted choice > chat capability. */
export function resolveInitialPaneUiMode(
  paneId: string,
  opts: { chatCapable: boolean },
): PaneUiMode {
  const explicit = getClientPaneUiMode(paneId);
  if (explicit) return explicit === "chat" && opts.chatCapable ? "chat" : "terminal";
  return opts.chatCapable ? "chat" : "terminal";
}

export function getClientPaneUiMode(paneId: string): PaneUiMode | null {
  const key = `${PREFIX}${paneId}`;
  try {
    const value = window.localStorage.getItem(key);
    if (value === "chat" || value === "terminal") return value;
  } catch {
    /* localStorage may be disabled; fall back to sessionStorage. */
  }
  try {
    const value = window.sessionStorage.getItem(key);
    return value === "chat" || value === "terminal" ? value : null;
  } catch {
    return null;
  }
}

export function setClientPaneUiMode(paneId: string, mode: PaneUiMode): void {
  currentModes.set(paneId, mode);
  const key = `${PREFIX}${paneId}`;
  try {
    window.localStorage.setItem(key, mode);
  } catch {
    /* localStorage may be disabled; sessionStorage still covers this webview. */
  }
  try {
    window.sessionStorage.setItem(key, mode);
  } catch {
    /* sessionStorage may be disabled; the event still updates this client. */
  }
  window.dispatchEvent(
    new CustomEvent(EVENT, {
      detail: { paneId, mode },
    }),
  );
}

export function listenClientPaneUiMode(
  paneId: string,
  onMode: (mode: PaneUiMode) => void,
): () => void {
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<{ paneId?: string; mode?: PaneUiMode }>).detail;
    if (detail?.paneId !== paneId) return;
    if (detail.mode === "chat" || detail.mode === "terminal") onMode(detail.mode);
  };
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
}

/** Status feed: fires on both explicit choices and published current modes.
 *  For display consumers (tab labels); never record these as user overrides. */
export function listenCurrentPaneUiMode(
  paneId: string,
  onMode: (mode: PaneUiMode) => void,
): () => void {
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<{ paneId?: string; mode?: PaneUiMode }>).detail;
    if (detail?.paneId !== paneId) return;
    if (detail.mode === "chat" || detail.mode === "terminal") onMode(detail.mode);
  };
  window.addEventListener(EVENT, handler);
  window.addEventListener(EVENT_CURRENT, handler);
  return () => {
    window.removeEventListener(EVENT, handler);
    window.removeEventListener(EVENT_CURRENT, handler);
  };
}
