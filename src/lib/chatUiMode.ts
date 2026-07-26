import { getAgentDescriptor } from "./agentDescriptorCache";
import type { PaneBacking } from "./types";

export type PaneUiMode = "terminal" | "chat";

export function isChatUiSupported(agentType?: string | null, backing?: PaneBacking | null): boolean {
  if (backing?.kind === "chat_backend") return true;
  if (!agentType) return false;
  return getAgentDescriptor(agentType)?.supports_chat_ui === true;
}

export function shouldDefaultNewAgentPaneToChat(
  agentType?: string | null,
  backing?: PaneBacking | null,
): boolean {
  return isChatUiSupported(agentType, backing);
}

export function emitPaneUiMode(paneId: string, mode: PaneUiMode): void {
  window.dispatchEvent(new CustomEvent("codeterm-pane-ui-mode", {
    detail: { paneId, mode },
  }));
  if (mode === "terminal") {
    requestAnimationFrame(() => {
      window.dispatchEvent(new CustomEvent("codeterm-pane-resize", { detail: { paneId, force: true } }));
    });
  }
}

export function isPaneUiModeEvent(event: Event): event is CustomEvent<{ paneId: string; mode: PaneUiMode }> {
  const detail = (event as CustomEvent).detail;
  return Boolean(
    detail &&
    typeof detail.paneId === "string" &&
    (detail.mode === "terminal" || detail.mode === "chat"),
  );
}
