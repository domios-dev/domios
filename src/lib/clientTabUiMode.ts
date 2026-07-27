import type { TabUiMode } from "./chatUiMode";

const PREFIX = "codeterm:tab-ui-mode:";
// default, remote-control) — TerminalTab records those as user overrides.
const EVENT = "codeterm-client-tab-ui-mode";
const EVENT_CURRENT = "codeterm-client-tab-ui-mode-current";
const currentModes = new Map<string, TabUiMode>();

export function getCurrentTabUiMode(tabId: string): TabUiMode | null {
  return currentModes.get(tabId) ?? null;
}

export function publishCurrentTabUiMode(tabId: string, mode: TabUiMode): void {
  currentModes.set(tabId, mode);
  window.dispatchEvent(
    new CustomEvent(EVENT_CURRENT, {
      detail: { tabId, mode },
    }),
  );
}

/** Single source of truth for a tab's opening view, shared by the tab body and
 *  the mode switch so they agree on the first frame (no terminal→chat flicker).
 *  Priority: explicit persisted choice > chat capability. */
export function resolveInitialTabUiMode(
  tabId: string,
  opts: { chatCapable: boolean },
): TabUiMode {
  const explicit = getClientTabUiMode(tabId);
  if (explicit) return explicit === "chat" && opts.chatCapable ? "chat" : "terminal";
  return opts.chatCapable ? "chat" : "terminal";
}

export function getClientTabUiMode(tabId: string): TabUiMode | null {
  const key = `${PREFIX}${tabId}`;
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

export function setClientTabUiMode(tabId: string, mode: TabUiMode): void {
  currentModes.set(tabId, mode);
  const key = `${PREFIX}${tabId}`;
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
      detail: { tabId, mode },
    }),
  );
}

export function listenClientTabUiMode(
  tabId: string,
  onMode: (mode: TabUiMode) => void,
): () => void {
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<{ tabId?: string; mode?: TabUiMode }>).detail;
    if (detail?.tabId !== tabId) return;
    if (detail.mode === "chat" || detail.mode === "terminal") onMode(detail.mode);
  };
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
}

/** Status feed: fires on both explicit choices and published current modes.
 *  For display consumers (tab labels); never record these as user overrides. */
export function listenCurrentTabUiMode(
  tabId: string,
  onMode: (mode: TabUiMode) => void,
): () => void {
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<{ tabId?: string; mode?: TabUiMode }>).detail;
    if (detail?.tabId !== tabId) return;
    if (detail.mode === "chat" || detail.mode === "terminal") onMode(detail.mode);
  };
  window.addEventListener(EVENT, handler);
  window.addEventListener(EVENT_CURRENT, handler);
  return () => {
    window.removeEventListener(EVENT, handler);
    window.removeEventListener(EVENT_CURRENT, handler);
  };
}
