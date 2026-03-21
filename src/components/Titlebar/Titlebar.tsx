import { useCallback, useMemo } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useMultiplexer } from "../../store/multiplexer";
import { NotificationBadge } from "../Notifications/NotificationBadge";
import type { PaneStatus, ThemeConfig } from "../../lib/types";

const STATUS_COLORS: Record<PaneStatus, string> = {
  Working: "#7ee787",
  Waiting: "#d29922",
  Idle: "var(--ct-muted)",
  Dead: "var(--ct-red)",
};

interface TitlebarProps {
  onToggleSidebar?: () => void;
  onNewTab?: () => void;
  workspaceName?: string;
  tabTitle?: string;
  theme?: ThemeConfig;
}

export function Titlebar({
  onToggleSidebar,
  onNewTab,
  workspaceName,
  tabTitle,
  theme: _theme,
}: TitlebarProps) {
  // theme is available for future dynamic titlebar coloring
  void _theme;
  const appWindow = getCurrentWindow();
  const { state } = useMultiplexer();

  const unreadCount = useMemo(
    () => state.notifications.filter((n) => !n.read).length,
    [state.notifications],
  );

  const focusedStatus: PaneStatus | undefined =
    state.focused_pane_id
      ? state.paneStatuses[state.focused_pane_id]
      : undefined;

  const handleMinimize = useCallback(() => {
    appWindow.minimize();
  }, [appWindow]);

  const handleMaximize = useCallback(() => {
    appWindow.toggleMaximize();
  }, [appWindow]);

  const handleClose = useCallback(() => {
    appWindow.close();
  }, [appWindow]);

  return (
    <div
      className="flex items-center h-8 bg-[var(--ct-titlebar-bg)] border-b border-[var(--ct-border)] select-none shrink-0"
      data-tauri-drag-region
    >
      {/* Left: sidebar toggle + title */}
      <div className="flex items-center gap-2 pl-3" data-tauri-drag-region>
        <button
          onClick={onToggleSidebar}
          className="flex items-center justify-center w-6 h-6 rounded hover:bg-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-colors"
          title="Toggle sidebar"
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
            <path d="M1 2.75A.75.75 0 0 1 1.75 2h12.5a.75.75 0 0 1 0 1.5H1.75A.75.75 0 0 1 1 2.75Zm0 5A.75.75 0 0 1 1.75 7h12.5a.75.75 0 0 1 0 1.5H1.75A.75.75 0 0 1 1 7.75ZM1.75 12h12.5a.75.75 0 0 1 0 1.5H1.75a.75.75 0 0 1 0-1.5Z" />
          </svg>
        </button>
        <span
          className="text-xs font-semibold text-[var(--ct-muted-fg)] tracking-wide"
          data-tauri-drag-region
        >
          CodeTerm
        </span>

        {workspaceName && (
          <>
            <span className="text-[10px] text-[var(--ct-muted)]" data-tauri-drag-region>/</span>
            <span
              className="text-xs text-[var(--ct-muted-fg)]"
              data-tauri-drag-region
            >
              {workspaceName}
            </span>
          </>
        )}
        {tabTitle && (
          <>
            <span className="text-[10px] text-[var(--ct-muted)]" data-tauri-drag-region>/</span>
            <span
              className="text-xs text-[var(--ct-fg)]"
              data-tauri-drag-region
            >
              {tabTitle}
            </span>
          </>
        )}
      </div>

      {/* Spacer -- drag region */}
      <div className="flex-1" data-tauri-drag-region />

      {/* Right: window controls */}
      <div className="flex items-center h-full">
        <button
          onClick={handleMinimize}
          className="inline-flex items-center justify-center w-12 h-full hover:bg-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-colors"
          title="Minimize"
        >
          <svg width="10" height="1" viewBox="0 0 10 1" fill="currentColor">
            <rect width="10" height="1" />
          </svg>
        </button>
        <button
          onClick={handleMaximize}
          className="inline-flex items-center justify-center w-12 h-full hover:bg-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] transition-colors"
          title="Maximize"
        >
          <svg
            width="10"
            height="10"
            viewBox="0 0 10 10"
            fill="none"
            stroke="currentColor"
            strokeWidth="1"
          >
            <rect x="0.5" y="0.5" width="9" height="9" />
          </svg>
        </button>
        <button
          onClick={handleClose}
          className="inline-flex items-center justify-center w-12 h-full hover:bg-[var(--ct-red)] text-[var(--ct-muted-fg)] hover:text-white transition-colors"
          title="Close"
        >
          <svg width="10" height="10" viewBox="0 0 10 10" fill="currentColor">
            <path d="M1.41 0L5 3.59 8.59 0 10 1.41 6.41 5 10 8.59 8.59 10 5 6.41 1.41 10 0 8.59 3.59 5 0 1.41Z" />
          </svg>
        </button>
      </div>
    </div>
  );
}
