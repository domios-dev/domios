import { getAgentDescriptor } from "./agentDescriptorCache";
import { invoke } from "./bridge";
import { loadConfig, setUiSettings } from "./clientSettings";
import type { AppConfig, PaneBacking } from "./types";

export const CHAT_UI_DEFAULT_KEY = "codeterm-chat-ui-default-enabled";
export const CHAT_UI_DEFAULT_PROMPT_EVENT = "codeterm-chat-ui-default-prompt";

export type PaneUiMode = "terminal" | "chat";
export type ChatUiDefaultPromptChoice = "yes" | "no";
type ChatUiDefaultPromptResult = ChatUiDefaultPromptChoice | "unhandled";
export type ChatUiDefaultPromptDetail = {
  handled: boolean;
  choose: (choice: ChatUiDefaultPromptChoice) => void;
};

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

// "Have we asked the user the chat-UI-default question yet?" — centralized in
// backend config so a fresh client never re-prompts. Test-overridable.
let promptedStoreForTest: { get: () => boolean; set: () => void } | null = null;

async function isPrompted(): Promise<boolean> {
  if (promptedStoreForTest) return promptedStoreForTest.get();
  try {
    const config = await loadConfig();
    return config.ui?.chat_ui_default_prompted === true;
  } catch {
    return false;
  }
}

async function markPrompted(): Promise<void> {
  if (promptedStoreForTest) { promptedStoreForTest.set(); return; }
  await setUiSettings({ chat_ui_default_prompted: true });
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
  await writeDefaultToConfig(enabled);
  cachedDefaultNewAgentPaneChat = enabled;
  configLoad = Promise.resolve(enabled);
  await markPrompted();
}

function askDefaultNewAgentPaneToChat(): Promise<ChatUiDefaultPromptResult> {
  return new Promise((resolve) => {
    const detail: ChatUiDefaultPromptDetail = {
      handled: false,
      choose: resolve,
    };
    window.dispatchEvent(new CustomEvent(CHAT_UI_DEFAULT_PROMPT_EVENT, { detail }));
    if (!detail.handled) {
      resolve("unhandled");
    }
  });
}

export async function maybeAskDefaultNewAgentPaneToChat(): Promise<void> {
  if (await isPrompted()) return;
  const choice = await askDefaultNewAgentPaneToChat();
  if (choice === "unhandled") return;
  if (choice === "yes") {
    await setDefaultNewAgentPaneToChat(true);
  } else {
    await markPrompted();
  }
}

export function __setChatUiDefaultConfigWriterForTest(
  writer: ((enabled: boolean) => Promise<void>) | null,
): void {
  configWriterForTest = writer;
}

export function __setChatUiDefaultPromptedStoreForTest(
  store: { get: () => boolean; set: () => void } | null,
): void {
  promptedStoreForTest = store;
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
