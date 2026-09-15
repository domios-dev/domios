import { useCallback, useEffect, useState } from "react";
import { TabModeSwitch } from "./TabModeSwitch";
import { UsageBubble } from "../Usage/UsageBubble";
import { PluginBubbles } from "../Extensions/PluginBubbles";
import type { ApiConfig, Tab } from "../../lib/types";
import { TAB_BAR_CONTROL_H } from "./tabBarConstants";
import { WatcherStatusBadge } from "./WatcherStatusBadge";
import { TabActionsMenu } from "./TabActionsMenu";
import { matchesBinding } from "../../lib/keybindings";
import { usePluginTabMenu } from "../../lib/pluginTabMenu";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { reconnectTabSession, restartTabSession, tabSessionActionView } from "./tabSessionActions";
import { buildTabMenu, type TabMenuAction, type TabMenuContext } from "./tabMenuModel";
import { performTabMenuAction } from "./tabMenuActions";
import { useTelegramThreadMenu } from "./useTelegramThreadMenu";

/** Single height rhythm for every control in the bar (switch + pills). */
export { TAB_BAR_CONTROL_H };

export interface HeaderTabMenu {
  tab: Tab;
  context: TabMenuContext;
  onRename?: (title: string) => void;
  onClose?: () => void;
}

interface TabTopBarProps {
  tabId: string;
  cwd: string | null;
  /** Show the Terminal | Chat switch (chat-capable PTY tabs only). */
  showSwitch: boolean;
  /** Usage provider id, or null to omit the usage pill. */
  usageProvider?: string | null;
  usageVisible?: boolean;
  api?: ApiConfig;
  /** Watcher tabs surface health from `tab.watcher.last_status` in the header. */
  watcherLastStatus?: string | null;
  isWatcherTab?: boolean;
  isActive?: boolean;
  tabMenu?: HeaderTabMenu;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function TabRenameField({ title, onDone }: { title: string; onDone: (title: string | null) => void }) {
  const [value, setValue] = useState(title);
  return (
    <input
      autoFocus
      aria-label={`Rename tab ${title}`}
      data-testid="tab-header-rename"
      value={value}
      onChange={(event) => setValue(event.target.value)}
      onBlur={() => {
        const trimmed = value.trim();
        onDone(trimmed && trimmed !== title ? trimmed : null);
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") event.currentTarget.blur();
        else if (event.key === "Escape") onDone(null);
      }}
      className="min-w-0 rounded border border-[var(--ct-border)] bg-[var(--ct-bg)] px-1.5 text-[12px] text-[var(--ct-fg)] outline-none"
      style={{ height: TAB_BAR_CONTROL_H }}
    />
  );
}

/**
 * Tab header strip: mode switch on the left, status pills (usage + plugins) on
 * the right. Every item shares `TAB_BAR_CONTROL_H` — published to CSS-driven
 * children via the `--ct-tab-pill-h` custom property.
 */
export function TabTopBar({
  tabId,
  cwd,
  showSwitch,
  usageProvider,
  usageVisible,
  watcherLastStatus,
  isWatcherTab = false,
  isActive = false,
  tabMenu,
}: TabTopBarProps) {
  const [reconnecting, setReconnecting] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [reconnectError, setReconnectError] = useState<string | null>(null);
  const menuTab = tabMenu?.tab ?? null;
  const onRename = tabMenu?.onRename;
  const canReconnect = menuTab ? tabSessionActionView(menuTab).canReconnect : false;
  const pluginTabMenu = usePluginTabMenu();
  const telegramThread = useTelegramThreadMenu(tabId, menuTab !== null && !menuTab.remote_peer_id);

  const reconnect = useCallback(async () => {
    if (reconnecting) return;
    setReconnecting(true);
    setReconnectError(null);
    try {
      await reconnectTabSession(tabId);
    } catch (error) {
      setReconnectError(errorMessage(error));
    } finally {
      setReconnecting(false);
    }
  }, [tabId, reconnecting]);

  const restartSession = useCallback(async () => {
    if (restarting) return;
    setRestarting(true);
    setReconnectError(null);
    try {
      await restartTabSession(tabId);
    } catch (error) {
      setReconnectError(errorMessage(error));
    } finally {
      setRestarting(false);
    }
  }, [tabId, restarting]);

  useEffect(() => {
    if (!isActive || !canReconnect) return;
    const handleReloadShortcut = (event: KeyboardEvent) => {
      if (!matchesBinding(event, "Ctrl+R")) return;
      event.preventDefault();
      event.stopPropagation();
      void reconnect();
    };
    window.addEventListener("keydown", handleReloadShortcut, true);
    return () => window.removeEventListener("keydown", handleReloadShortcut, true);
  }, [canReconnect, isActive, reconnect]);

  const entries = tabMenu
    ? buildTabMenu({
      ...tabMenu.context,
      tab: tabMenu.tab,
      surface: "header",
      canRename: Boolean(tabMenu.onRename),
      canClose: Boolean(tabMenu.onClose),
      pluginTabMenu,
      telegramThread: telegramThread.state,
      busy: { reconnecting, restarting },
    })
    : [];

  const runMenuAction = (action: TabMenuAction) => {
    if (action.type === "reconnect") {
      void reconnect();
      return;
    }
    if (action.type === "restart") {
      void restartSession();
      return;
    }
    setReconnectError(null);
    performTabMenuAction(tabId, action, { rename: () => setRenaming(true), close: tabMenu?.onClose })
      .catch((error) => setReconnectError(errorMessage(error)));
  };

  return (
    <div
      data-testid="tab-top-bar"
      className="flex min-w-0 shrink-0 flex-wrap items-center justify-between gap-1.5 px-2 sm:flex-nowrap sm:gap-2"
      style={{
        ["--ct-tab-pill-h" as string]: `${TAB_BAR_CONTROL_H}px`,
        paddingTop: 6,
        paddingBottom: 6,
        background: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 3%, transparent)",
        borderBottom: "1px solid var(--ct-border, #30363d)",
      }}
    >
      <div className="flex min-w-0 shrink items-center gap-1.5 sm:gap-2">
        {renaming && menuTab && onRename ? (
          <TabRenameField
            title={displayTitleForTab(menuTab)}
            onDone={(title) => {
              setRenaming(false);
              if (title) onRename(title);
            }}
          />
        ) : showSwitch && (
          <TabModeSwitch
            tabId={tabId}
            height={TAB_BAR_CONTROL_H}
            chatPty={showSwitch}
          />
        )}
      </div>
      <div className="ml-auto flex min-w-0 shrink items-center gap-1 sm:gap-1.5">
        {reconnectError && (
          <span
            data-testid="chat-reconnect-error"
            className="max-w-48 truncate text-[10px]"
            style={{ color: "var(--ct-red)" }}
          >
            {reconnectError}
          </span>
        )}
        {isWatcherTab && <WatcherStatusBadge lastStatus={watcherLastStatus} />}
        <PluginBubbles cwd={cwd} tabId={tabId} isActive={isActive} />
        {usageProvider && <UsageBubble provider={usageProvider} trackingEnabled={!!usageVisible} />}
        {entries.length > 0 && (
          <TabActionsMenu
            entries={entries}
            busy={reconnecting || restarting}
            onAction={runMenuAction}
            onOpen={telegramThread.refresh}
          />
        )}
      </div>
    </div>
  );
}
