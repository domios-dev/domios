import { pluginTabMenuEntries, type PluginTabMenuEntry, type PluginTabMenuState } from "../../lib/pluginTabMenu";
import { canMakeOrchestrator } from "../../lib/orchestratorOnboard";
import { isPlanner } from "../../lib/roleKind";
import type { ProviderDescriptor, Tab, TabStatus } from "../../lib/types";
import type { TabTabActivity } from "../Canvas/tabTabActivity";
import { orchestratorRecoveryMenuView, type OrchestratorRecoveryMenuView } from "../Sidebar/orchestratorRecoveryMenu";
import { tabModelEntryLabel, tabModelMenuView, type TabModelMenuView } from "../Sidebar/tabModelMenu";
import { tabProviderEntryLabel, tabProviderMenuView } from "../Sidebar/tabProviderMenu";
import {
  permissionAutoConfirmMenuState,
  TAB_SESSION_LABELS,
  tabSessionActionView,
  type TabSessionActionView,
} from "./tabSessionActions";
import { TELEGRAM_THREAD_LABELS, telegramThreadHint, type TelegramThreadMenuState } from "./telegramThreadActions";

export type TabMenuSurface = "sidebar" | "header";

interface SurfaceCapabilities {
  autoConfirm: boolean;
  reconnectShortcut: string | null;
}

const SURFACES: Record<TabMenuSurface, SurfaceCapabilities> = {
  // Owner decision: the long auto-confirm toggle stays out of the compact sidebar menu.
  sidebar: { autoConfirm: false, reconnectShortcut: null },
  // Ctrl+R reconnects only the tab whose header is visible.
  header: { autoConfirm: true, reconnectShortcut: "Ctrl+R" },
};

export const TAB_MENU_LABELS = {
  menu: "Tab actions",
  rename: "Rename",
  makeOrchestrator: "Make Orchestrator",
  makePlanner: "Make Planner",
  replaceOrchestrator: "Replace Orchestrator",
  replaceOrchestratorHint: "Starts a new Team owner without resuming this session.",
  providerHint: "Restarts the session — the new provider starts fresh.",
  plugins: "Plugins",
  closeSession: "Close Session",
  closeTab: "Close Tab",
} as const;

function modelHint(providerName: string): string {
  return `Switches in session — ${providerName} keeps this session and its history.`;
}

export type TabMenuAction =
  | { type: "rename" }
  | { type: "makeOrchestrator" }
  | { type: "makePlanner"; planId: string }
  | { type: "switchModel"; view: TabModelMenuView; modelId: string }
  | { type: "switchProvider"; providerId: string }
  | { type: "replaceOrchestrator"; view: OrchestratorRecoveryMenuView; providerId: string }
  | { type: "reconnect" }
  | { type: "restart" }
  | { type: "copySessionId"; sessionId: string }
  | { type: "setAutoConfirm"; enabled: boolean }
  | { type: "runPlugin"; entry: PluginTabMenuEntry }
  | { type: "openTelegram" }
  | { type: "closeTelegram" }
  | { type: "close" };

export type TabMenuGlyph =
  | "rename"
  | "role"
  | "model"
  | "provider"
  | "reconnect"
  | "restart"
  | "autoConfirm"
  | "plugins"
  | "telegram"
  | "telegramClose"
  | "close";

export type TabMenuIcon = TabMenuGlyph | { svg: string | null; color: string | null } | null;

interface TabMenuEntryBase {
  id: string;
  testId?: string;
}

export interface TabMenuHeaderEntry extends TabMenuEntryBase {
  kind: "header";
  sessionId: string | null;
  copyAction: TabMenuAction | null;
}

export interface TabMenuItemEntry extends TabMenuEntryBase {
  kind: "item";
  label: string;
  icon: TabMenuIcon;
  action: TabMenuAction | null;
  selected?: boolean;
  disabled?: boolean;
  unavailable?: boolean;
  hint?: string;
  title?: string;
  danger?: boolean;
  shortcut?: string;
}

export interface TabMenuCheckEntry extends TabMenuEntryBase {
  kind: "check";
  label: string;
  icon: TabMenuIcon;
  checked: boolean;
  description?: string;
  keepOpen?: boolean;
  action: TabMenuAction;
}

export interface TabMenuSeparatorEntry extends TabMenuEntryBase {
  kind: "separator";
}

export type TabMenuLeafEntry = TabMenuItemEntry | TabMenuCheckEntry | TabMenuSeparatorEntry;

export interface TabMenuSubmenuEntry extends TabMenuEntryBase {
  kind: "submenu";
  label: string;
  icon: TabMenuIcon;
  description?: string;
  disabled?: boolean;
  title?: string;
  entries: TabMenuLeafEntry[];
}

export type TabMenuEntry = TabMenuHeaderEntry | TabMenuLeafEntry | TabMenuSubmenuEntry;

export interface TabMenuContext {
  status: TabStatus | null;
  descriptor: ProviderDescriptor | null | undefined;
  descriptors: ProviderDescriptor[];
  activity: TabTabActivity | null;
  orchestrators: Tab[] | undefined;
}

export interface TabMenuInput extends TabMenuContext {
  tab: Tab;
  surface: TabMenuSurface;
  canRename: boolean;
  canClose: boolean;
  pluginTabMenu: PluginTabMenuState;
  telegramThread: TelegramThreadMenuState | null;
  busy?: { reconnecting: boolean; restarting: boolean };
}

function modelSubmenu({ tab, descriptor, status }: TabMenuInput): TabMenuSubmenuEntry | null {
  const view = tabModelMenuView(tab, descriptor, status);
  if (!view) return null;
  return {
    kind: "submenu",
    id: "model",
    testId: "tab-menu-model",
    label: tabModelEntryLabel(view),
    icon: "model",
    disabled: Boolean(view.blockedReason),
    title: view.blockedReason ?? undefined,
    description: modelHint(descriptor?.display_name || "Agent"),
    entries: view.models.map((model): TabMenuItemEntry => ({
      kind: "item",
      id: `model-${model.id}`,
      testId: `tab-menu-model-${model.id}`,
      label: model.display_name,
      icon: null,
      selected: model.id === view.currentModel?.id,
      action: { type: "switchModel", view, modelId: model.id },
    })),
  };
}

function providerSubmenu({ tab, descriptors, status }: TabMenuInput): TabMenuSubmenuEntry | null {
  const view = tabProviderMenuView(tab, descriptors, status);
  if (!view) return null;
  return {
    kind: "submenu",
    id: "provider",
    testId: "tab-menu-provider",
    label: tabProviderEntryLabel(view),
    icon: "provider",
    disabled: Boolean(view.blockedReason),
    title: view.blockedReason ?? undefined,
    description: TAB_MENU_LABELS.providerHint,
    entries: view.providers.map((provider): TabMenuItemEntry => ({
      kind: "item",
      id: `provider-${provider.id}`,
      testId: `tab-menu-provider-${provider.id}`,
      label: provider.display_name,
      icon: null,
      selected: provider.id === view.currentProviderId,
      action: provider.id === view.currentProviderId ? null : { type: "switchProvider", providerId: provider.id },
    })),
  };
}

function recoverySubmenu({ tab, descriptors, orchestrators }: TabMenuInput): TabMenuSubmenuEntry | null {
  const view = orchestratorRecoveryMenuView(tab, descriptors, orchestrators);
  if (!view) return null;
  return {
    kind: "submenu",
    id: "replace-orchestrator",
    testId: "tab-menu-replace-orchestrator",
    label: TAB_MENU_LABELS.replaceOrchestrator,
    icon: "role",
    description: TAB_MENU_LABELS.replaceOrchestratorHint,
    entries: view.providers.map((candidate): TabMenuItemEntry => ({
      kind: "item",
      id: `replace-orchestrator-${candidate.id}`,
      testId: `tab-menu-replace-orchestrator-${candidate.id}`,
      label: candidate.display_name,
      icon: "provider",
      action: { type: "replaceOrchestrator", view, providerId: candidate.id },
    })),
  };
}

function sessionItems(input: TabMenuInput, session: TabSessionActionView): TabMenuItemEntry[] {
  const reconnecting = input.busy?.reconnecting === true;
  const restarting = input.busy?.restarting === true;
  const busy = reconnecting || restarting;
  const shortcut = SURFACES[input.surface].reconnectShortcut;
  const items: TabMenuItemEntry[] = [];
  if (session.canReconnect) {
    items.push({
      kind: "item",
      id: "reconnect",
      testId: "chat-reconnect",
      label: reconnecting ? TAB_SESSION_LABELS.reconnecting : TAB_SESSION_LABELS.reconnect,
      icon: "reconnect",
      disabled: busy,
      ...(shortcut ? { shortcut } : {}),
      action: { type: "reconnect" },
    });
  }
  if (session.canRestart) {
    items.push({
      kind: "item",
      id: "restart",
      testId: "tab-menu-restart-session",
      label: restarting ? TAB_SESSION_LABELS.restarting : TAB_SESSION_LABELS.restart,
      icon: "restart",
      disabled: busy,
      action: { type: "restart" },
    });
  }
  return items;
}

function autoConfirmEntries(input: TabMenuInput): TabMenuLeafEntry[] {
  if (!SURFACES[input.surface].autoConfirm) return [];
  const enabled = permissionAutoConfirmMenuState(input.tab, input.descriptor);
  if (enabled === null) return [];
  return [
    { kind: "separator", id: "auto-confirm-separator" },
    {
      kind: "check",
      id: "auto-confirm",
      testId: "tab-permission-auto-confirm",
      label: TAB_SESSION_LABELS.autoConfirm,
      description: TAB_SESSION_LABELS.autoConfirmHint,
      icon: "autoConfirm",
      checked: enabled,
      keepOpen: true,
      action: { type: "setAutoConfirm", enabled: !enabled },
    },
  ];
}

function telegramEntries(state: TelegramThreadMenuState | null): TabMenuItemEntry[] {
  const hint = state ? telegramThreadHint(state) : null;
  const open: TabMenuItemEntry = {
    kind: "item",
    id: "telegram-open",
    testId: "tab-menu-telegram-connect",
    label: TELEGRAM_THREAD_LABELS.open,
    icon: "telegram",
    unavailable: state !== null && !state.bridgeActive,
    ...(hint ? { hint } : {}),
    action: { type: "openTelegram" },
  };
  if (!state?.bound) return [open];
  return [open, {
    kind: "item",
    id: "telegram-close",
    testId: "tab-menu-telegram-disconnect",
    label: TELEGRAM_THREAD_LABELS.close,
    icon: "telegramClose",
    action: { type: "closeTelegram" },
  }];
}

function pluginsSubmenu({ tab, pluginTabMenu, telegramThread }: TabMenuInput): TabMenuSubmenuEntry | null {
  // A peer's tab has no local plugin receiver or bridge thread binding.
  if (tab.remote_peer_id) return null;
  const entries: TabMenuLeafEntry[] = [
    ...pluginTabMenuEntries(pluginTabMenu, tab.id).map((entry): TabMenuCheckEntry => ({
      kind: "check",
      id: `plugin-${entry.pluginId}-${entry.itemId}`,
      testId: `tab-menu-plugin-${entry.pluginId}-${entry.itemId}`,
      label: entry.label,
      icon: { svg: entry.iconSvg ?? null, color: entry.iconColor ?? null },
      checked: entry.checked,
      action: { type: "runPlugin", entry },
    })),
    ...telegramEntries(telegramThread),
  ];
  if (entries.length === 0) return null;
  return {
    kind: "submenu",
    id: "plugins",
    testId: "tab-menu-plugins",
    label: TAB_MENU_LABELS.plugins,
    icon: "plugins",
    entries,
  };
}

export function buildTabMenu(input: TabMenuInput): TabMenuEntry[] {
  const { tab, activity } = input;
  const session = tabSessionActionView(tab);
  const entries: TabMenuEntry[] = [];
  if (session.canReconnect) {
    entries.push({
      kind: "header",
      id: "session",
      testId: "tab-menu-copy-session-id",
      sessionId: session.sessionId,
      copyAction: session.sessionId ? { type: "copySessionId", sessionId: session.sessionId } : null,
    });
  }
  if (input.canRename && !tab.protected) {
    entries.push({ kind: "item", id: "rename", testId: "tab-menu-rename", label: TAB_MENU_LABELS.rename, icon: "rename", action: { type: "rename" } });
  }
  if (canMakeOrchestrator(tab)) {
    entries.push({
      kind: "item",
      id: "make-orchestrator",
      testId: "tab-menu-make-orchestrator",
      label: TAB_MENU_LABELS.makeOrchestrator,
      icon: "role",
      action: { type: "makeOrchestrator" },
    });
  }
  if (activity?.kind === "worker" && !isPlanner(tab)) {
    entries.push({
      kind: "item",
      id: "make-planner",
      testId: "tab-menu-make-planner",
      label: TAB_MENU_LABELS.makePlanner,
      icon: "role",
      action: { type: "makePlanner", planId: activity.planId },
    });
  }
  for (const submenu of [modelSubmenu(input), providerSubmenu(input), recoverySubmenu(input)]) {
    if (submenu) entries.push(submenu);
  }
  entries.push(...sessionItems(input, session), ...autoConfirmEntries(input));
  const plugins = pluginsSubmenu(input);
  if (plugins) entries.push(plugins);
  if (input.canClose && !tab.protected) {
    entries.push({
      kind: "item",
      id: "close",
      testId: "tab-menu-close",
      label: tab.agent_type != null ? TAB_MENU_LABELS.closeSession : TAB_MENU_LABELS.closeTab,
      icon: "close",
      danger: true,
      action: { type: "close" },
    });
  }
  return entries;
}
