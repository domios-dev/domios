import type { PaneUiMode } from "./chatUiMode";

const PREFIX = "codeterm:pane-ui-mode:";
const TERMINAL_OVERRIDES_NORMALIZED_KEY =
  "codeterm:pane-ui-mode:terminal-overrides-normalized-for-chat-default";
// Two channels on purpose. EVENT carries EXPLICIT choices (user toggle, spawn
// default, remote-control) — TerminalPane records those as user overrides.
const EVENT = "codeterm-client-pane-ui-mode";
const EVENT_CURRENT = "codeterm-client-pane-ui-mode-current";
const EVENT_DEFAULT_CHANGED = "codeterm-client-pane-ui-mode-default-changed";
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
 *  Priority: explicit persisted choice > backend chat_view > global chat-default. */
export function resolveInitialPaneUiMode(
  paneId: string,
  opts: { chatCapable: boolean; chatViewFlag?: boolean; chatDefault?: boolean; mobileDefault?: boolean },
): PaneUiMode {
  const explicit = getClientPaneUiMode(paneId);
  if (explicit) return explicit === "chat" && opts.chatCapable ? "chat" : "terminal";
  if (!opts.chatCapable) return "terminal";
  if (opts.chatViewFlag) return "chat";
  if (opts.chatDefault) return "chat";
  return opts.mobileDefault ? "chat" : "terminal";
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

function readStorageValue(storage: Storage, key: string): string | null {
  try {
    return storage.getItem(key);
  } catch {
    return null;
  }
}

function removeStorageValue(storage: Storage, key: string): void {
  try {
    storage.removeItem(key);
  } catch {
    /* storage may be unavailable in tests or hardened webviews. */
  }
}

function forEachPaneUiModeStorageKey(storage: Storage, fn: (key: string, paneId: string) => void): void {
  let keys: string[] = [];
  try {
    keys = Array.from({ length: storage.length }, (_, i) => storage.key(i)).filter((key): key is string =>
      typeof key === "string" && key.startsWith(PREFIX),
    );
  } catch {
    return;
  }
  for (const key of keys) fn(key, key.slice(PREFIX.length));
}

export function clearTerminalClientPaneUiModeOverrides(): string[] {
  const cleared = new Set<string>();
  for (const [paneId, mode] of currentModes.entries()) {
    if (mode !== "terminal") continue;
    currentModes.delete(paneId);
    cleared.add(paneId);
  }
  for (const storage of [window.localStorage, window.sessionStorage]) {
    forEachPaneUiModeStorageKey(storage, (key, paneId) => {
      if (readStorageValue(storage, key) !== "terminal") return;
      removeStorageValue(storage, key);
      cleared.add(paneId);
    });
  }
  return [...cleared];
}

export function markTerminalPaneUiModeOverridesNormalizedForChatDefault(): void {
  try {
    window.localStorage.setItem(TERMINAL_OVERRIDES_NORMALIZED_KEY, "true");
  } catch {
    /* localStorage may be unavailable; normalization still applies in-memory. */
  }
}

export function resetTerminalPaneUiModeOverrideNormalization(): void {
  try {
    window.localStorage.removeItem(TERMINAL_OVERRIDES_NORMALIZED_KEY);
  } catch {
    /* localStorage may be unavailable. */
  }
}

export function normalizeTerminalPaneUiModeOverridesForChatDefault(): string[] {
  try {
    if (window.localStorage.getItem(TERMINAL_OVERRIDES_NORMALIZED_KEY) === "true") return [];
  } catch {
    /* fall through and clear in-memory/session state. */
  }
  const cleared = clearTerminalClientPaneUiModeOverrides();
  markTerminalPaneUiModeOverridesNormalizedForChatDefault();
  return cleared;
}

export function publishPaneUiModeDefaultChanged(enabled: boolean): void {
  window.dispatchEvent(
    new CustomEvent(EVENT_DEFAULT_CHANGED, {
      detail: { enabled },
    }),
  );
}

export function listenPaneUiModeDefaultChanged(
  onDefaultChanged: (enabled: boolean) => void,
): () => void {
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<{ enabled?: boolean }>).detail;
    if (typeof detail?.enabled === "boolean") onDefaultChanged(detail.enabled);
  };
  window.addEventListener(EVENT_DEFAULT_CHANGED, handler);
  return () => window.removeEventListener(EVENT_DEFAULT_CHANGED, handler);
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
