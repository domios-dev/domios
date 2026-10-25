import { useCallback, useState, useEffect, type Ref } from "react";
import { nativeBridge } from "../../lib/nativeBridge";
import type { ApiConfig, ThemeConfig } from "../../lib/types";
import { CloseXIcon, HamburgerIcon } from "../Icons";

import { isMac } from "../../lib/keybindings";
import { IS_WEB } from "../../lib/webApi";
import { UI_BUTTON, UI_TRAY_DIVIDER } from "../shared/design";
import { KeepAwakeTray } from "./KeepAwakeTray";
import { ExtensionsTray } from "./ExtensionsTray";
import { AnalyticsTray } from "./AnalyticsTray";
import { AccountTray } from "./AccountTray";
import { SettingsMenu } from "./SettingsMenu";
import type { SettingsLinkId } from "../Settings/sections";
import { NotificationToastContainer } from "../Notifications/NotificationToast";
import type { Notification } from "../../lib/types";
import { NotificationBell } from "../Notifications/NotificationBell";
import { developmentBuildLabel } from "./buildIdentity";
import { DevBuildDot } from "./DevBuildDot";
import "./Titlebar.css";
import { WebPeerPicker } from "../WebPeerPicker/WebPeerPicker";
import { FirstRunControls } from "../AgentPicker/FirstRunControls";

interface TitlebarProps {
  locked?: boolean;
  api?: ApiConfig;
  onToggleSidebar?: () => void;
  showSidebarToggle?: boolean;
  onNewTab?: () => void;
  theme: ThemeConfig;
  onOpenExtensions?: () => void;
  analyticsOpen?: boolean;
  onToggleAnalytics?: () => void;
  analyticsButtonRef?: Ref<HTMLButtonElement>;
  onOpenSettings?: (section?: SettingsLinkId) => void;
  notifications?: Notification[];
  notificationsLoaded?: boolean;
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
  locked = false,
  onToggleSidebar,
  showSidebarToggle = false,
  theme: _theme,
  onOpenExtensions,
  analyticsOpen = false,
  onToggleAnalytics,
  analyticsButtonRef,
  onOpenSettings,
  notifications = [],
  notificationsLoaded = true,
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
  const isTauri = nativeBridge.window !== undefined;
  const appWindow = nativeBridge.window;
  const handleMinimize = useCallback(() => appWindow?.minimize(), [appWindow]);
  const handleMaximize = useCallback(() => appWindow?.toggleMaximize(), [appWindow]);
  const handleClose = useCallback(() => appWindow?.close(), [appWindow]);
  const buildLabel = developmentBuildLabel(__CODETERM_DEV__, __APP_VERSION__);

  return (
    <FirstRunControls locked={locked}>
    <div
      data-testid="titlebar"
      className={`ct-titlebar flex min-w-0 items-center select-none shrink-0 ${IS_WEB ? "relative z-50 max-sm:min-h-[calc(44px+var(--ct-safe-top,0px))]" : ""}`}
      style={{
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

      {IS_WEB && (
        <div className="flex h-full shrink-0 items-center px-1 sm:px-2">
          <WebPeerPicker compact />
        </div>
      )}

      <div
        className="hidden h-full shrink-0 items-center gap-2 pl-3 pr-2 min-[430px]:flex"
        data-tauri-drag-region
      >
        {buildLabel && <DevBuildDot label={buildLabel} version={__APP_VERSION__} />}
      </div>

      <div className="ml-auto flex shrink-0 items-center gap-0.5 pr-2 sm:gap-1 sm:pr-1.5">
        <div className="relative flex items-center">
          {onDismissToast && <NotificationToastContainer toasts={toasts} onDismiss={onDismissToast} onMarkRead={onMarkNotificationRead} onNavigate={onNavigateToast} />}
          {showBell && onMarkNotificationRead && onMarkAllNotificationsRead && onDeleteNotification && onClearNotifications && onNavigateToast && (
            <NotificationBell
              notifications={notifications}
              loading={!notificationsLoaded}
              onMarkRead={onMarkNotificationRead}
              onMarkAllRead={onMarkAllNotificationsRead}
              onDelete={onDeleteNotification}
              onClearAll={onClearNotifications}
              onNavigate={onNavigateToast}
              onAnswer={onAnswerNotification}
            />
          )}
        </div>
        {onToggleAnalytics && (
          <AnalyticsTray ref={analyticsButtonRef} open={analyticsOpen} onToggle={onToggleAnalytics} />
        )}
        {onOpenExtensions && <ExtensionsTray onOpen={onOpenExtensions} />}
        <KeepAwakeTray />
        {onOpenSettings && (
          <>
            <span className={UI_TRAY_DIVIDER} aria-hidden />
            <SettingsMenu onOpenSection={onOpenSettings} afterGear={<span data-first-run-allowed="true" style={{ display: "contents" }}><AccountTray locked={locked} onOpenPlans={() => { if (!locked) onOpenSettings("plans"); }} /></span>} />
          </>
        )}
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
    </FirstRunControls>
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
        style={{ backgroundColor: dim ? "var(--ct-muted)" : "var(--ct-window-close)" }}
        title="Close"
      >
        {hover && (
          <svg width="6" height="6" viewBox="0 0 6 6" fill="none" stroke="var(--ct-window-close-glyph)" strokeWidth="1.2" strokeLinecap="round">
            <line x1="1" y1="1" x2="5" y2="5" />
            <line x1="5" y1="1" x2="1" y2="5" />
          </svg>
        )}
      </button>
      {/* Minimize — yellow */}
      <button
        onClick={onMinimize}
        className="w-3 h-3 rounded-full flex items-center justify-center transition-colors"
        style={{ backgroundColor: dim ? "var(--ct-muted)" : "var(--ct-window-minimize)" }}
        title="Minimize"
      >
        {hover && (
          <svg width="6" height="2" viewBox="0 0 6 2" fill="none" stroke="var(--ct-window-minimize-glyph)" strokeWidth="1.2" strokeLinecap="round">
            <line x1="1" y1="1" x2="5" y2="1" />
          </svg>
        )}
      </button>
      {/* Maximize — green */}
      <button
        onClick={onMaximize}
        className="w-3 h-3 rounded-full flex items-center justify-center transition-colors"
        style={{ backgroundColor: dim ? "var(--ct-muted)" : "var(--ct-window-maximize)" }}
        title="Maximize"
      >
        {hover && (
          <svg width="6" height="6" viewBox="0 0 6 6" fill="none" stroke="var(--ct-window-maximize-glyph)" strokeWidth="1.1" strokeLinecap="round">
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
        className={`${UI_BUTTON.base} h-full w-12 text-[var(--ct-muted)] hover:text-white hover:bg-[var(--ct-window-close-hover)] transition-colors`} 
        title="Close"
      >
        <CloseXIcon />
      </button>
    </div>
  );
}
