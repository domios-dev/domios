import { useCallback, useState, useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ThemeConfig } from "../../lib/types";
import { CloseXIcon, HamburgerIcon } from "../Icons";
import logoUrl from "../../assets/logo.png";

import { isMac } from "../../lib/keybindings";
import { IS_WEB } from "../../lib/webApi";
import { UI_BUTTON, UI_TRAY_DIVIDER } from "../shared/design";
import { KeepAwakeTray } from "./KeepAwakeTray";
import { ExtensionsTray } from "./ExtensionsTray";
import { SettingsMenu } from "./SettingsMenu";
import type { SectionId } from "../Settings/sections";
import { NotificationToastContainer } from "../Notifications/NotificationToast";
import type { Notification } from "../../lib/types";
import { NotificationBell } from "../Notifications/NotificationBell";
import { developmentBuildLabel } from "./buildIdentity";

interface TitlebarProps {
  onToggleSidebar?: () => void;
  showSidebarToggle?: boolean;
  onNewTab?: () => void;
  theme: ThemeConfig;
  onOpenExtensions?: () => void;
  onOpenSettings?: (section?: SectionId) => void;
  notifications?: Notification[];
  /** The master bell switch. False removes the indicator; nothing about what is
   *  recorded changes, so flipping it back reveals the complete log. */
  showBell?: boolean;
  toasts?: Notification[];
  onMarkNotificationRead?: (id: string) => void;
  onMarkAllNotificationsRead?: () => void;
  onDeleteNotification?: (id: string) => void;
  onClearNotifications?: () => void;
  onAnswerNotification?: (id: string, label: string) => void;
  onDismissToast?: (id: string) => void;
  onNavigateToast?: (tabId: string, workspaceId: string | null) => void;
}

export function Titlebar({
  onToggleSidebar,
  showSidebarToggle = false,
  theme: _theme,
  onOpenExtensions,
  onOpenSettings,
  notifications = [],
  showBell = true,
  toasts = [],
  onMarkNotificationRead,
  onMarkAllNotificationsRead,
  onDeleteNotification,
  onClearNotifications,
  onAnswerNotification,
  onDismissToast,
  onNavigateToast,
}: TitlebarProps) {
  void _theme;
  // Window chrome controls only exist in the Tauri shell; the browser tab owns its own window in web-mirror mode.
  const isTauri = !IS_WEB;
  const appWindow = isTauri ? getCurrentWindow() : null;
  const handleMinimize = useCallback(() => appWindow?.minimize(), [appWindow]);
  const handleMaximize = useCallback(() => appWindow?.toggleMaximize(), [appWindow]);
  const handleClose = useCallback(() => appWindow?.close(), [appWindow]);
  const buildLabel = developmentBuildLabel(__CODETERM_DEV__, __APP_VERSION__);

  return (
    <div
      data-testid="titlebar"
      className={`flex min-w-0 items-center bg-[var(--ct-titlebar-bg)] select-none shrink-0 ${IS_WEB ? "max-sm:min-h-[calc(44px+var(--ct-safe-top,0px))]" : ""}`}
      style={{
        boxShadow: "inset 0 -0.5px 0 var(--ct-border-subtle)",
        height: "calc(2rem + var(--ct-safe-top, 0px))",
        paddingTop: "var(--ct-safe-top, 0px)",
      }}
      data-tauri-drag-region
    >
      {/* macOS: traffic lights on the left */}
      {isTauri && isMac && (
        <MacTrafficLights
          onClose={handleClose}
          onMinimize={handleMinimize}
          onMaximize={handleMaximize}
        />
      )}
      {showSidebarToggle && onToggleSidebar && (
        <button
          type="button"
          data-testid="web-sidebar-open"
          aria-label="Open sidebar"
          title="Open sidebar"
          onClick={onToggleSidebar}
          className="inline-flex h-full w-8 shrink-0 items-center justify-center text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
        >
          <HamburgerIcon size={15} />
        </button>
      )}

      <div
        className="hidden h-full shrink-0 items-center gap-2 pl-3 pr-2 min-[430px]:flex"
        data-tauri-drag-region
      >
        <img
          src={logoUrl}
          alt=""
          className="h-4 w-4 rounded-sm opacity-90"
        />
        <span
          className="text-[11px] font-medium text-[var(--ct-muted-fg)]"
        >
          CodeTerm
        </span>
        {buildLabel && (
          <span
            className="rounded bg-[var(--ct-accent)] px-1.5 py-0.5 font-mono text-[8px] font-bold leading-none tracking-wide text-[var(--ct-bg)]"
            aria-label={`Development build, version ${__APP_VERSION__}`}
          >
            {buildLabel}
          </span>
        )}
      </div>

      <div className="min-w-0 flex-1" data-tauri-drag-region />

      <div className="flex min-w-0 flex-1 items-center justify-end h-full overflow-hidden">
        {onDismissToast && (
          <NotificationToastContainer
            toasts={toasts}
            onDismiss={onDismissToast}
            onMarkRead={onMarkNotificationRead}
            onNavigate={onNavigateToast}
            embedded
          />
        )}
        <div className="flex shrink-0 items-center gap-0.5 pr-1 sm:gap-1 sm:pr-1.5">
          {showBell && onMarkNotificationRead && onMarkAllNotificationsRead && onDeleteNotification && onClearNotifications && onNavigateToast && (
            <NotificationBell
              notifications={notifications}
              onMarkRead={onMarkNotificationRead}
              onMarkAllRead={onMarkAllNotificationsRead}
              onDelete={onDeleteNotification}
              onClearAll={onClearNotifications}
              onNavigate={onNavigateToast}
              onAnswer={onAnswerNotification}
            />
          )}
          {onOpenExtensions && <ExtensionsTray onOpen={onOpenExtensions} />}
          <KeepAwakeTray />
          {onOpenSettings && (
            <>
              <span className={UI_TRAY_DIVIDER} aria-hidden />
              <SettingsMenu onOpenSection={onOpenSettings} />
            </>
          )}
        </div>
      </div>

      {/* Windows/Linux: controls on the right */}
      {isTauri && !isMac && (
        <WindowsControls
          onClose={handleClose}
          onMinimize={handleMinimize}
          onMaximize={handleMaximize}
        />
      )}
    </div>
  );
}

// ── macOS Traffic Lights ──────────────────────────────────────────────

function MacTrafficLights({
  onClose,
  onMinimize,
  onMaximize,
}: {
  onClose: () => void;
  onMinimize: () => void;
  onMaximize: () => void;
}) {
  const [hover, setHover] = useState(false);

  const [focused, setFocused] = useState(true);
  useEffect(() => {
    const onFocus = () => setFocused(true);
    const onBlur = () => setFocused(false);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
    };
  }, []);

  const dim = !focused && !hover;

  return (
    <div
      className="flex items-center gap-2 pl-3.5 pr-1 h-full"
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
    >
      {/* Close — red */}
      <button
        onClick={onClose}
        className="w-3 h-3 rounded-full flex items-center justify-center transition-colors"
        style={{ backgroundColor: dim ? "var(--ct-muted)" : "#ff5f57" }}
        title="Close"
      >
        {hover && (
          <svg width="6" height="6" viewBox="0 0 6 6" fill="none" stroke="#4d0000" strokeWidth="1.2" strokeLinecap="round">
            <line x1="1" y1="1" x2="5" y2="5" />
            <line x1="5" y1="1" x2="1" y2="5" />
          </svg>
        )}
      </button>
      {/* Minimize — yellow */}
      <button
        onClick={onMinimize}
        className="w-3 h-3 rounded-full flex items-center justify-center transition-colors"
        style={{ backgroundColor: dim ? "var(--ct-muted)" : "#febc2e" }}
        title="Minimize"
      >
        {hover && (
          <svg width="6" height="2" viewBox="0 0 6 2" fill="none" stroke="#995700" strokeWidth="1.2" strokeLinecap="round">
            <line x1="1" y1="1" x2="5" y2="1" />
          </svg>
        )}
      </button>
      {/* Maximize — green */}
      <button
        onClick={onMaximize}
        className="w-3 h-3 rounded-full flex items-center justify-center transition-colors"
        style={{ backgroundColor: dim ? "var(--ct-muted)" : "#28c840" }}
        title="Maximize"
      >
        {hover && (
          <svg width="6" height="6" viewBox="0 0 6 6" fill="none" stroke="#006500" strokeWidth="1.1" strokeLinecap="round">
            <polyline points="1,3.5 1,1 3.5,1" />
            <polyline points="5,2.5 5,5 2.5,5" />
          </svg>
        )}
      </button>
    </div>
  );
}

// ── Windows/Linux Controls ────────────────────────────────────────────

function WindowsControls({
  onClose,
  onMinimize,
  onMaximize,
}: {
  onClose: () => void;
  onMinimize: () => void;
  onMaximize: () => void;
}) {
  return (
    <div className="flex items-center h-full">
      <button
        onClick={onMinimize}
        className={`${UI_BUTTON.ghost} h-full w-12`} 
        title="Minimize"
      >
        <svg width="10" height="1" viewBox="0 0 10 1" fill="currentColor">
          <rect width="10" height="1" />
        </svg>
      </button>
      <button
        onClick={onMaximize}
        className={`${UI_BUTTON.ghost} h-full w-12`}
        title="Maximize"
      >
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
          <rect x="0.5" y="0.5" width="9" height="9" />
        </svg>
      </button>
      <button
        onClick={onClose}
        className={`${UI_BUTTON.base} h-full w-12 text-[var(--ct-muted)] hover:text-white hover:bg-[#e81123] transition-colors`} 
        title="Close"
      >
        <CloseXIcon />
      </button>
    </div>
  );
}
