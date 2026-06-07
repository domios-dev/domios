import { useCallback, useState, useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebPeerPicker } from "../WebPeerPicker/WebPeerPicker";
import type { ThemeConfig } from "../../lib/types";
import { HamburgerIcon, CloseXIcon } from "../Icons";

import { isMac } from "../../lib/keybindings";
import { KeepAwakeTray } from "./KeepAwakeTray";
import { MeshPresenceTray } from "./MeshPresenceTray";
import { UpdateBadge } from "../UpdateBadge";
import { UI_BUTTON } from "../shared/design";

interface TitlebarProps {
  onToggleSidebar?: () => void;
  onNewTab?: () => void;
  theme?: ThemeConfig;
  onOpenSettingsAbout?: () => void;
}

export function Titlebar({
  onToggleSidebar,
  theme: _theme,
  onOpenSettingsAbout,
}: TitlebarProps) {
  void _theme;
  // Window chrome controls only exist in the Tauri shell; the browser tab owns its own window in web-mirror mode.
  const isTauri = "__TAURI_INTERNALS__" in window;
  const appWindow = isTauri ? getCurrentWindow() : null;
  const handleMinimize = useCallback(() => appWindow?.minimize(), [appWindow]);
  const handleMaximize = useCallback(() => appWindow?.toggleMaximize(), [appWindow]);
  const handleClose = useCallback(() => appWindow?.close(), [appWindow]);

  return (
    <div
      data-testid="titlebar"
      className="flex items-center h-10 bg-[var(--ct-titlebar-bg)] select-none shrink-0"
      style={{ boxShadow: "inset 0 -0.5px 0 var(--ct-border-subtle)" }}
      data-tauri-drag-region
    >
      {/* macOS: traffic lights on the left */}
      {isMac && (
        <MacTrafficLights
          onClose={handleClose}
          onMinimize={handleMinimize}
          onMaximize={handleMaximize}
        />
      )}

      {/* Left: sidebar toggle + breadcrumb */}
      <div className="flex items-center gap-1.5 pl-3" data-tauri-drag-region>
        <button
          onClick={onToggleSidebar}
          className={`${UI_BUTTON.ghost} w-9 h-9`}
          title="Toggle sidebar"
        >
          <HamburgerIcon size={15} />
        </button>

        {/* Web mirror: peer switcher, top-left per spec */}
        {!isTauri && <WebPeerPicker />}

        <span
          className="ml-1 text-[11px] text-[var(--ct-muted)]"
          data-tauri-drag-region
        >
          CodeTerm
        </span>
      </div>

      {/* Spacer — drag region */}
      <div className="flex-1" data-tauri-drag-region />

      {/* Tray area — right side, before window controls */}
      <div className="flex items-center gap-1 pr-1">
        <UpdateBadge onNavigate={onOpenSettingsAbout} />
        <MeshPresenceTray />
        <KeepAwakeTray />
      </div>

      {/* Windows/Linux: controls on the right */}
      {!isMac && (
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

  // Track window focus for dimming
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

