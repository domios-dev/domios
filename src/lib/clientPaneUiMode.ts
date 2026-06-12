import type { PaneUiMode } from "./chatUiMode";

const PREFIX = "codeterm:pane-ui-mode:";
// Two channels on purpose. EVENT carries EXPLICIT choices (user toggle, spawn
// default, remote-control) — TerminalPane records those as user overrides.
// EVENT_CURRENT is a status broadcast of a pane's computed mode (for tab
// labels etc.); routing it through EVENT made TerminalPane hear its own
// mount-time "terminal" echo and lock it as an override, freezing panes in
// terminal before descriptors/backend state arrived.
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
