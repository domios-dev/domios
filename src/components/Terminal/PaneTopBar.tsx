import { useState } from "react";
import { RefreshCw } from "lucide-react";
import { PaneModeSwitch } from "./PaneModeSwitch";
import { ChatDefaultSuggestion } from "./ChatDefaultSuggestion";
import { UsageBubble } from "../Usage/UsageBubble";
import { PluginBubbles } from "../Extensions/PluginBubbles";
import type { ApiConfig } from "../../lib/types";
import { PANE_BAR_CONTROL_H } from "./paneBarConstants";
import { WatcherStatusBadge } from "./WatcherStatusBadge";
import { reattachPaneChat } from "./chatReattach";

/** Single height rhythm for every control in the bar (switch + pills). */
export { PANE_BAR_CONTROL_H };

interface PaneTopBarProps {
  paneId: string;
  cwd: string | null;
  /** Show the Terminal | Chat switch (chat-capable PTY panes only). */
  showSwitch: boolean;
  /** Chat is the default view → lead the picker with Chat. From App's config. */
  chatFirst: boolean;
  /** Whether the chat-default nudge was already answered. From App's config. */
  chatDefaultPrompted: boolean;
  /** Usage provider id, or null to omit the usage pill. */
  usageProvider?: string | null;
  usageVisible?: boolean;
  api?: ApiConfig;
  /** Watcher panes surface health from `pane.watcher.last_status` in the header. */
  watcherLastStatus?: string | null;
  isWatcherPane?: boolean;
  showChatReconnect?: boolean;
}

/**
 * Pane header strip: mode switch on the left, status pills (usage + plugins) on
 * the right. Every item shares `PANE_BAR_CONTROL_H` — published to CSS-driven
 * children via the `--ct-pane-pill-h` custom property.
 *
 * Config-derived flags (`chatFirst`, `chatDefaultPrompted`) come in as props from
 * App's single useConfig — a per-pane useConfig here multiplied get_config by pane
 * count and froze pane switches (see App.tsx note on the prior UsageBubble fix).
 */
export function PaneTopBar({
  paneId,
  cwd,
  showSwitch,
  chatFirst,
  chatDefaultPrompted,
  usageProvider,
  usageVisible,
  watcherLastStatus,
  isWatcherPane = false,
  showChatReconnect = false,
}: PaneTopBarProps) {
  const [reconnecting, setReconnecting] = useState(false);
  const [reconnectError, setReconnectError] = useState<string | null>(null);

  const reconnect = async () => {
    if (reconnecting) return;
    setReconnecting(true);
    setReconnectError(null);
    try {
      await reattachPaneChat(paneId);
    } catch (error) {
      setReconnectError(error instanceof Error ? error.message : String(error));
    } finally {
      setReconnecting(false);
    }
  };

  return (
    <div
      data-testid="pane-top-bar"
      className="flex shrink-0 items-center justify-between gap-2 px-2"
      style={{
        ["--ct-pane-pill-h" as string]: `${PANE_BAR_CONTROL_H}px`,
        paddingTop: "calc(6px + var(--ct-safe-top, 0px))",
        paddingBottom: 6,
        background: "color-mix(in srgb, var(--ct-fg, #c9d1d9) 3%, transparent)",
        borderBottom: "1px solid var(--ct-border, #30363d)",
      }}
    >
      <div className="flex min-w-0 items-center gap-2">
        {showSwitch && (
          <PaneModeSwitch
            paneId={paneId}
            height={PANE_BAR_CONTROL_H}
            chatFirst={chatFirst}
            chatPty={showSwitch}
          />
        )}
        {showSwitch && (
          <ChatDefaultSuggestion
            paneId={paneId}
            height={PANE_BAR_CONTROL_H}
            prompted={chatDefaultPrompted}
            isDefault={chatFirst}
          />
        )}
      </div>
      <div className="flex min-w-0 items-center gap-1.5">
        {reconnectError && (
          <span
            data-testid="chat-reconnect-error"
            className="max-w-48 truncate text-[10px]"
            style={{ color: "var(--ct-red)" }}
          >
            {reconnectError}
          </span>
        )}
        {showChatReconnect && (
          <button
            type="button"
            data-testid="chat-reconnect"
            aria-label="Reconnect chat session"
            title={reconnectError ?? "Reconnect chat session"}
            disabled={reconnecting}
            onClick={() => void reconnect()}
            className="inline-flex items-center gap-1.5 rounded px-2 text-xs hover:brightness-125 disabled:opacity-60"
            style={{
              height: PANE_BAR_CONTROL_H,
              border: "1px solid var(--ct-border)",
              color: "var(--ct-fg)",
            }}
          >
            <RefreshCw size={13} className={reconnecting ? "animate-spin" : undefined} />
            <span>Reconnect</span>
          </button>
        )}
        {isWatcherPane && <WatcherStatusBadge lastStatus={watcherLastStatus} />}
        <PluginBubbles cwd={cwd} paneId={paneId} />
        {usageProvider && <UsageBubble provider={usageProvider} trackingEnabled={!!usageVisible} />}
      </div>
    </div>
  );
}
