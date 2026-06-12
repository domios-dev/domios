import { getAgentDescriptor } from "./agentDescriptorCache";
import { invoke } from "./bridge";
import type { AppConfig } from "./types";

export const CHAT_UI_DEFAULT_KEY = "codeterm-chat-ui-default-enabled";
export const CHAT_UI_DEFAULT_PROMPTED_KEY = "codeterm-chat-ui-default-prompted";

export type PaneUiMode = "terminal" | "chat";

// Capability-driven: a provider supports chat when its descriptor says so
// (JSONL sessions or structured/DB-backed chat). No hardcoded provider list.
export function isChatUiSupported(agentType?: string | null): boolean {
  if (!agentType) return false;
  return getAgentDescriptor(agentType)?.supports_chat_ui === true;
}

export function shouldDefaultNewAgentPaneToChat(agentType?: string | null): boolean {
  if (!isChatUiSupported(agentType)) return false;
  return cachedDefaultNewAgentPaneChat;
}

let cachedDefaultNewAgentPaneChat = false;
let configLoad: Promise<boolean> | null = null;

function readLegacyDefault(): boolean | null {
  try {
    const value = localStorage.getItem(CHAT_UI_DEFAULT_KEY);
    return value === null ? null : value === "true";
  } catch {
    return null;
  }
}

function markPrompted(): void {
  try {
    localStorage.setItem(CHAT_UI_DEFAULT_PROMPTED_KEY, "true");
  } catch {
    // localStorage can be unavailable in tests or hardened webviews.
  }
}

function clearLegacyDefault(): void {
  try {
    localStorage.removeItem(CHAT_UI_DEFAULT_KEY);
  } catch {
    // localStorage can be unavailable in tests or hardened webviews.
  }
}

async function writeDefaultToConfig(enabled: boolean): Promise<void> {
  const config = await invoke<AppConfig>("get_config");
  await invoke("update_config", {
    config: {
      ...config,
      ui: {
        ...config.ui,
        default_new_agent_pane_chat: enabled,
      },
    },
  });
}

export function loadDefaultNewAgentPaneChat(): Promise<boolean> {
  if (configLoad) return configLoad;
  configLoad = (async () => {
    const config = await invoke<AppConfig>("get_config");
    const legacy = readLegacyDefault();
    if (legacy !== null) {
      cachedDefaultNewAgentPaneChat = legacy;
      await invoke("update_config", {
        config: {
          ...config,
          ui: {
            ...config.ui,
            default_new_agent_pane_chat: legacy,
          },
        },
      });
      clearLegacyDefault();
      return legacy;
    }
    cachedDefaultNewAgentPaneChat = config.ui?.default_new_agent_pane_chat === true;
    return cachedDefaultNewAgentPaneChat;
  })().catch((err) => {
    configLoad = null;
    throw err;
  });
  return configLoad;
}

export async function setDefaultNewAgentPaneToChat(enabled: boolean): Promise<void> {
  cachedDefaultNewAgentPaneChat = enabled;
  markPrompted();
  await writeDefaultToConfig(enabled);
}

export async function maybeAskDefaultNewAgentPaneToChat(): Promise<void> {
  try {
    if (localStorage.getItem(CHAT_UI_DEFAULT_PROMPTED_KEY) === "true") return;
  } catch {
    return;
  }
  markPrompted();
  if (window.confirm("Use Chat UI by default for new supported agent panes?")) {
    await setDefaultNewAgentPaneToChat(true);
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
