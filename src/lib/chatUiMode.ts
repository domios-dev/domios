import { getAgentDescriptor } from "./agentDescriptorCache";
import { invoke } from "./bridge";
import type { AppConfig, PaneBacking } from "./types";

export const CHAT_UI_DEFAULT_KEY = "codeterm-chat-ui-default-enabled";
export const CHAT_UI_DEFAULT_PROMPTED_KEY = "codeterm-chat-ui-default-prompted";
export const CHAT_UI_DEFAULT_PROMPT_EVENT = "codeterm-chat-ui-default-prompt";

export type PaneUiMode = "terminal" | "chat";
export type ChatUiDefaultPromptChoice = "yes" | "no";
export type ChatUiDefaultPromptDetail = {
  handled: boolean;
  choose: (choice: ChatUiDefaultPromptChoice) => void;
};

// Capability-driven: a provider supports chat when its descriptor says so
// (JSONL sessions or structured/DB-backed chat). chatBackend shell panes have no
// `agent_type` — they are intrinsically chat, so the backing alone qualifies.
// No hardcoded provider list either way.
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

let cachedDefaultNewAgentPaneChat = false;
let configLoad: Promise<boolean> | null = null;
let configWriterForTest: ((enabled: boolean) => Promise<void>) | null = null;

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
  if (configWriterForTest) {
    await configWriterForTest(enabled);
    return;
  }
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

function askDefaultNewAgentPaneToChat(): Promise<ChatUiDefaultPromptChoice> {
  return new Promise((resolve) => {
    const detail: ChatUiDefaultPromptDetail = {
      handled: false,
      choose: resolve,
    };
    window.dispatchEvent(new CustomEvent(CHAT_UI_DEFAULT_PROMPT_EVENT, { detail }));
    if (!detail.handled) {
      resolve("no");
    }
  });
}

export async function maybeAskDefaultNewAgentPaneToChat(): Promise<void> {
  try {
    if (localStorage.getItem(CHAT_UI_DEFAULT_PROMPTED_KEY) === "true") return;
  } catch {
    return;
  }
  const choice = await askDefaultNewAgentPaneToChat();
  if (choice === "yes") {
    await setDefaultNewAgentPaneToChat(true);
  } else {
    markPrompted();
  }
}

export function __setChatUiDefaultConfigWriterForTest(
  writer: ((enabled: boolean) => Promise<void>) | null,
): void {
  configWriterForTest = writer;
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
