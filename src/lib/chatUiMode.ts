const CHAT_UI_SUPPORTED_PROVIDERS = new Set(["claude", "codex"]);

export const CHAT_UI_DEFAULT_KEY = "codeterm-chat-ui-default-enabled";
export const CHAT_UI_DEFAULT_PROMPTED_KEY = "codeterm-chat-ui-default-prompted";

export type PaneUiMode = "terminal" | "chat";

export function isChatUiSupported(agentType?: string | null): boolean {
  return Boolean(agentType && CHAT_UI_SUPPORTED_PROVIDERS.has(agentType));
}

export function shouldDefaultNewAgentPaneToChat(agentType?: string | null): boolean {
  if (!isChatUiSupported(agentType)) return false;
  try {
    return localStorage.getItem(CHAT_UI_DEFAULT_KEY) === "true";
  } catch {
    return false;
  }
}

export function setDefaultNewAgentPaneToChat(enabled: boolean): void {
  try {
    localStorage.setItem(CHAT_UI_DEFAULT_KEY, String(enabled));
    localStorage.setItem(CHAT_UI_DEFAULT_PROMPTED_KEY, "true");
  } catch {
    // localStorage can be unavailable in tests or hardened webviews.
  }
}

export function maybeAskDefaultNewAgentPaneToChat(): void {
  try {
    if (localStorage.getItem(CHAT_UI_DEFAULT_PROMPTED_KEY) === "true") return;
    localStorage.setItem(CHAT_UI_DEFAULT_PROMPTED_KEY, "true");
    if (window.confirm("Use Chat UI by default for new supported agent panes?")) {
      localStorage.setItem(CHAT_UI_DEFAULT_KEY, "true");
    }
  } catch {
    // Ignore prompt/default persistence failures.
  }
}

export function emitPaneUiMode(paneId: string, mode: PaneUiMode): void {
  window.dispatchEvent(new CustomEvent("codeterm-pane-ui-mode", {
    detail: { paneId, mode },
  }));
}

export function isPaneUiModeEvent(event: Event): event is CustomEvent<{ paneId: string; mode: PaneUiMode }> {
  const detail = (event as CustomEvent).detail;
  return Boolean(
    detail &&
    typeof detail.paneId === "string" &&
    (detail.mode === "terminal" || detail.mode === "chat"),
  );
}
