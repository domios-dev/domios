import { getAgentDescriptor } from "./agentDescriptorCache";
import { invoke } from "./bridge";
import { setUiSettings } from "./clientSettings";
import type { AppConfig, PaneBacking } from "./types";

export const CHAT_UI_DEFAULT_KEY = "codeterm-chat-ui-default-enabled";
// Sync mirror of the chat-default so the FIRST render resolves the right view
// before the async get_config warms the cache — kills the terminal→chat flash.
const CHAT_DEFAULT_SYNC_KEY = "codeterm:chat-default-new-agent-pane";

function readChatDefaultSync(): boolean | null {
  try {
    const value = localStorage.getItem(CHAT_DEFAULT_SYNC_KEY);
    return value === null ? null : value === "true";
  } catch {
    return null;
  }
}

function persistChatDefaultSync(enabled: boolean): void {
  try {
    localStorage.setItem(CHAT_DEFAULT_SYNC_KEY, enabled ? "true" : "false");
  } catch {
    // localStorage may be unavailable; the in-memory cache still serves this session.
  }
}

export type PaneUiMode = "terminal" | "chat";

// Capability-driven: a provider supports chat when its descriptor says so
// (JSONL sessions or structured/DB-backed chat). chatBackend shell panes have no
export function isChatUiSupported(agentType?: string | null, backing?: PaneBacking | null): boolean {
  if (backing?.kind === "chat_backend") return true;
  if (!agentType) return false;
  return getAgentDescriptor(agentType)?.supports_chat_ui === true;
}

export function shouldDefaultNewAgentPaneToChat(
  agentType?: string | null,
  backing?: PaneBacking | null,
): boolean {
  if (backing?.kind === "chat_backend") return true; // shells are always chat
  if (!isChatUiSupported(agentType)) return false;
  return cachedDefaultNewAgentPaneChat;
}

let cachedDefaultNewAgentPaneChat = readChatDefaultSync() ?? false;
let configLoad: Promise<boolean> | null = null;

/** Synchronous read of the cached "new agent panes open in chat" setting.
 *  Warmed by loadDefaultNewAgentPaneChat() at startup — lets a pane pick its
 *  initial view without awaiting config or the descriptor cache. */
export function isChatDefaultEnabled(): boolean {
  return cachedDefaultNewAgentPaneChat;
}

function readLegacyDefault(): boolean | null {
  try {
    const value = localStorage.getItem(CHAT_UI_DEFAULT_KEY);
    return value === null ? null : value === "true";
  } catch {
    return null;
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
      persistChatDefaultSync(legacy);
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
    persistChatDefaultSync(cachedDefaultNewAgentPaneChat);
    return cachedDefaultNewAgentPaneChat;
  })().catch((err) => {
    configLoad = null;
    throw err;
  });
  return configLoad;
}

export async function setDefaultNewAgentPaneToChat(enabled: boolean): Promise<void> {
  await writeDefaultToConfig(enabled);
  cachedDefaultNewAgentPaneChat = enabled;
  persistChatDefaultSync(enabled);
  configLoad = Promise.resolve(enabled);
  await markChatDefaultPrompted();
}

// Records that the chat-default suggestion has been answered, so the inline
// prompt never reappears once the user accepts or dismisses it.
export async function markChatDefaultPrompted(): Promise<void> {
  await setUiSettings({ chat_ui_default_prompted: true });
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
