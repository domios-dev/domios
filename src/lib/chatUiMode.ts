import { getAgentDescriptor } from "./agentDescriptorCache";
import type { TabBacking } from "./types";

export type TabUiMode = "terminal" | "chat";

export function isChatUiSupported(agentType?: string | null, backing?: TabBacking | null): boolean {
  if (backing?.kind === "chat_backend") return true;
  if (!agentType) return false;
  return getAgentDescriptor(agentType)?.supports_chat_ui === true;
}

/** The tab body's own rule: an agent whose descriptor has not loaded yet is treated as chat-capable. */
export function tabChatCapable(tab: { agent_type?: string | null; backing?: TabBacking | null }): boolean {
  if (tab.backing?.kind === "chat_backend") return true;
  if (!tab.agent_type) return false;
  const descriptor = getAgentDescriptor(tab.agent_type);
  return descriptor ? descriptor.supports_chat_ui === true : true;
}

export function shouldDefaultNewAgentTabToChat(
  agentType?: string | null,
  backing?: TabBacking | null,
): boolean {
  return isChatUiSupported(agentType, backing);
}

export function emitTabUiMode(tabId: string, mode: TabUiMode): void {
  window.dispatchEvent(new CustomEvent("codeterm-tab-ui-mode", {
    detail: { tabId, mode },
  }));
  if (mode === "terminal") {
    requestAnimationFrame(() => {
      window.dispatchEvent(new CustomEvent("codeterm-tab-resize", { detail: { tabId, force: true } }));
    });
  }
}

export function isTabUiModeEvent(event: Event): event is CustomEvent<{ tabId: string; mode: TabUiMode }> {
  const detail = (event as CustomEvent).detail;
  return Boolean(
    detail &&
    typeof detail.tabId === "string" &&
    (detail.mode === "terminal" || detail.mode === "chat"),
  );
}
