import { makeOrchestrator, makePlanner } from "../../lib/orchestratorOnboard";
import { guardFreeLimit, openOnRefusal } from "../billing/freeLimitGate";
import {invoke} from "../../lib/bridge";
import { runPluginTabMenuItem } from "../../lib/pluginTabMenu";
import { replaceOrchestrator } from "../Sidebar/orchestratorRecoveryMenu";
import { switchTabModel } from "../Sidebar/tabModelMenu";
import type { TabMenuAction } from "./tabMenuModel";
import {
  copyTabSessionId,
  reconnectTabSession,
  restartTabSession,
  setTabPermissionAutoConfirm,
} from "./tabSessionActions";
import { closeTelegramThread, openTelegramThread } from "./telegramThreadActions";

export interface TabMenuSurfaceHandlers {
  sessionBindingNonce?: string | null;
  switchProvider?: (providerId: string) => void;
  rename?: () => void;
  close?: () => void;
}

export async function performTabMenuAction(
  tabId: string,
  action: TabMenuAction,
  handlers: TabMenuSurfaceHandlers = {},
): Promise<void> {
  switch (action.type) {
    case "rename":
      handlers.rename?.();
      return;
    case "close":
      handlers.close?.();
      return;
    case "makeOrchestrator":
      await openOnRefusal("teams", () => makeOrchestrator(tabId, handlers.sessionBindingNonce ?? undefined));
      return;
    case "makePlanner":
      await openOnRefusal("teams", () => makePlanner(tabId, action.planId, handlers.sessionBindingNonce ?? undefined));
      return;
    case "switchModel":
      return switchTabModel(action.view, action.modelId);
    case "switchProvider":
      if (!handlers.switchProvider) throw new Error("Provider switching requires the handoff confirmation dialog.");
      handlers.switchProvider(action.providerId);
      return;
    case "replaceOrchestrator":
      return replaceOrchestrator(action.view, action.providerId);
    case "reconnect":
      return reconnectTabSession(tabId);
    case "restart":
      return restartTabSession(tabId);
    case "fork":
      await guardFreeLimit(["agent.tabs"], () => invoke("fork_agent_tab",{tabId}));
      return;
    case "copySessionId":
      return copyTabSessionId(action.sessionId);
    case "setAutoConfirm":
      return setTabPermissionAutoConfirm(tabId, action.enabled);
    case "runPlugin":
      return runPluginTabMenuItem(action.entry, tabId);
    case "openTelegram":
      await openTelegramThread(tabId);
      return;
    case "closeTelegram":
      return closeTelegramThread(tabId);
  }
}
