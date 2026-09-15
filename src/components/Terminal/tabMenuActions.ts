import { makeOrchestrator, makePlanner } from "../../lib/orchestratorOnboard";
import { runPluginTabMenuItem } from "../../lib/pluginTabMenu";
import { replaceOrchestrator } from "../Sidebar/orchestratorRecoveryMenu";
import { getTabProviderApi } from "../Sidebar/tabProviderApi";
import { switchTabModel } from "../Sidebar/tabModelMenu";
import { switchTabProvider } from "../Sidebar/tabProviderMenu";
import type { TabMenuAction } from "./tabMenuModel";
import {
  copyTabSessionId,
  reconnectTabSession,
  restartTabSession,
  setTabPermissionAutoConfirm,
} from "./tabSessionActions";
import { closeTelegramThread, openTelegramThread } from "./telegramThreadActions";

export interface TabMenuSurfaceHandlers {
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
      return makeOrchestrator(tabId);
    case "makePlanner":
      return makePlanner(tabId, action.planId);
    case "switchModel":
      return switchTabModel(action.view, action.modelId);
    case "switchProvider":
      return switchTabProvider(getTabProviderApi(), tabId, action.providerId);
    case "replaceOrchestrator":
      return replaceOrchestrator(action.view, action.providerId);
    case "reconnect":
      return reconnectTabSession(tabId);
    case "restart":
      return restartTabSession(tabId);
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
