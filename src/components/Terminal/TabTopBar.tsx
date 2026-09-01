import { useCallback, useEffect, useState } from "react";
import { TabModeSwitch } from "./TabModeSwitch";
import { UsageBubble } from "../Usage/UsageBubble";
import { PluginBubbles } from "../Extensions/PluginBubbles";
import type { ApiConfig } from "../../lib/types";
import { TAB_BAR_CONTROL_H } from "./tabBarConstants";
import { WatcherStatusBadge } from "./WatcherStatusBadge";
import { TabActionsMenu } from "./TabActionsMenu";
import { matchesBinding } from "../../lib/keybindings";
import { reconnectTabSession, restartTabSession } from "./tabSessionActions";

/** Single height rhythm for every control in the bar (switch + pills). */
export { TAB_BAR_CONTROL_H };

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
  canReconnect?: boolean;
  canRestartSession?: boolean;
  sessionId?: string | null;
  isActive?: boolean;
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
  canReconnect = false,
  canRestartSession = false,
  sessionId = null,
  isActive = false,
}: TabTopBarProps) {
  const [reconnecting, setReconnecting] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [reconnectError, setReconnectError] = useState<string | null>(null);

  const reconnect = useCallback(async () => {
    if (reconnecting) return;
    setReconnecting(true);
    setReconnectError(null);
    try {
      await reconnectTabSession(tabId);
    } catch (error) {
      setReconnectError(error instanceof Error ? error.message : String(error));
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
      setReconnectError(error instanceof Error ? error.message : String(error));
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
        {showSwitch && (
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
        {(canReconnect || canRestartSession || sessionId) && (
          <TabActionsMenu
            sessionId={sessionId}
            canReconnect={canReconnect}
            canRestart={canRestartSession}
            reconnecting={reconnecting}
            restarting={restarting}
            onReconnect={() => void reconnect()}
            onRestart={() => void restartSession()}
            onError={setReconnectError}
          />
        )}
      </div>
    </div>
  );
}
