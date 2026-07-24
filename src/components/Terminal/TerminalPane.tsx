import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { SquareTerminal, X } from "lucide-react";
import { invoke } from "../../lib/bridge";
import { IS_WEB, listen } from "../../lib/bridge";
import { useMultiplexer, useMuxSelector } from "../../store/multiplexer";
import { paneFieldsEqual, selectPane } from "../../store/multiplexer/selectors";
import {
  useXterm,
  useTerminalResize,
  useShellIntegration,
  useTabCompletion,
  usePtyConnection,
  useTerminalInput,
  useAgentDetection,
  useTauriFileDrop,
  useTouchScroll,
} from "../../hooks/terminal";
import { quotePathForPaste, routeClipboardToTerm } from "../../lib/clipboardPaste";
import { registerPanePaste } from "../../lib/panePasteRegistry";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import {
  listenClientPaneUiMode,
  listenPaneUiModeDefaultChanged,
  publishCurrentPaneUiMode,
  resolveInitialPaneUiMode,
  setClientPaneUiMode,
} from "../../lib/clientPaneUiMode";
import { ContextMenu, type ContextMenuAction } from "./ContextMenu";
import { TerminalKeyBar } from "./TerminalKeyBar";
import { CompletionPopup } from "./CompletionPopup";
import { SearchBar } from "./SearchBar";
import { PaneSessionChat } from "./PaneSessionChat";
import {
  isChatDefaultEnabled,
  isPaneUiModeEvent,
  type PaneUiMode,
} from "../../lib/chatUiMode";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import type { ApiConfig, PaneStatus, ThemeConfig, FontConfig, TerminalConfigSettings } from "../../lib/types";
import { claimResizeOwner, getResizeClientId } from "../../lib/resizeOwner";
import { shouldShowDropOverlay } from "./dropOverlay";
import { terminalOwnsLivePty, terminalRenderEnabled } from "./terminalOwnership";
import { PaneBootOverlay, paneBootOverlayView } from "./bootOverlay";

interface TerminalPaneProps {
  paneId: string;
  ptyId?: string | null;
  isFocused?: boolean;
  isVisible?: boolean;
  hasSplits?: boolean;
  onFocus?: (paneId: string) => void;
  onPtySpawned?: (paneId: string, ptyId: string) => void;
  theme: ThemeConfig;
  fontConfig?: FontConfig;
  terminalConfig?: TerminalConfigSettings;
  uiFontScale?: number;
  api?: ApiConfig;
}

export const TerminalPane = memo(function TerminalPane({
  paneId,
  ptyId,
  isFocused = false,
  isVisible = true,
  hasSplits = false,
  onFocus,
  onPtySpawned,
  theme,
  fontConfig,
  terminalConfig,
  uiFontScale,
  api,
}: TerminalPaneProps) {
  const paneRootRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const fallbackShellRef = useRef<HTMLDivElement>(null);
  const connectedPtyRef = useRef<string | null>(null);

  const { updatePaneStatus } = useMultiplexer();
  const paneSelector = useCallback((s: Parameters<typeof selectPane>[0]) => selectPane(s, paneId), [paneId]);
  const pane = useMuxSelector(paneSelector, paneFieldsEqual);
  const agentType = pane?.agent_type;
  // panes and binds the session when the mux reports its id.
  const agentDescriptor = useAgentDescriptor(agentType);
  const chatCapable =
    pane?.backing?.kind === "chat_backend"
      ? true
      : !agentType
        ? false
        : agentDescriptor
          ? agentDescriptor.supports_chat_ui === true
          : true;
  const [mobileWebDefaultChat, setMobileWebDefaultChat] = useState(() =>
    IS_WEB && typeof window !== "undefined" && window.matchMedia("(max-width: 700px)").matches,
  );

  useEffect(() => {
    if (!IS_WEB || typeof window === "undefined") return;
    const query = window.matchMedia("(max-width: 700px)");
    const update = () => setMobileWebDefaultChat(query.matches);
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);
  const userModeOverrideRef = useRef<PaneUiMode | null>(null);
  const computeMode = useCallback((): PaneUiMode => {
    if (userModeOverrideRef.current) {
      return userModeOverrideRef.current === "chat" && chatCapable ? "chat" : "terminal";
    }
    return resolveInitialPaneUiMode(paneId, {
      chatCapable,
      chatViewFlag: pane?.chat_view === true,
      chatDefault: isChatDefaultEnabled(),
      mobileDefault: mobileWebDefaultChat,
    });
  }, [paneId, chatCapable, pane?.chat_view, mobileWebDefaultChat]);
  const [uiMode, setUiMode] = useState<PaneUiMode>(computeMode);
  const [terminalFallbackOpen, setTerminalFallbackOpen] = useState(false);
  const [historyReplaying, setHistoryReplaying] = useState(false);
  const [terminalIoReady, setTerminalIoReady] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const terminalVisible = uiMode === "terminal" || terminalFallbackOpen;
  const ownsLivePty = terminalOwnsLivePty({
    terminalVisible,
    paneVisible: isVisible,
    ptyId,
  });
  const renderTerminal = terminalRenderEnabled({
    terminalVisible,
    paneVisible: isVisible,
  });
  const { termRef, fitAddonRef, searchAddonRef, bellFlash, termReady } = useXterm({
    containerRef, theme, fontConfig, terminalConfig,
    priority: isVisible || isFocused,
    renderEnabled: renderTerminal,
  });
  const resizeClientId = useMemo(() => getResizeClientId(), []);
  const { commitResize } = useTerminalResize({
    containerRef,
    termRef,
    fitAddonRef,
    connectedPtyRef,
    ptyId,
    termReady,
    isFocused,
    resizeEnabled: terminalVisible,
    resizeClientId,
  });
  const claimTerminalOwnership = useCallback((force = false) => {
    if (!ownsLivePty && !force) return;
    const term = termRef.current;
    const ptyIdNow = connectedPtyRef.current ?? ptyId ?? null;
    if (!term || !ptyIdNow) return;
    if (term.cols <= 0 || term.rows <= 0) return;
    void claimResizeOwner(paneId, ptyIdNow, resizeClientId, term.cols, term.rows, force)
      .then(commitResize)
      .catch(() => {});
  }, [paneId, ptyId, resizeClientId, ownsLivePty, termRef, connectedPtyRef, commitResize]);

  const shell = useShellIntegration({
    termRef, connectedPtyRef, paneId, ptyId, termReady,
  });

  const completion = useTabCompletion({
    termRef, containerRef, connectedPtyRef,
  });

  const { writePty } = usePtyConnection({
    termRef, fitAddonRef, connectedPtyRef, paneId, ptyId, termReady,
    onPtySpawned,
    onShellReady: shell.markReady,
    onOutputChunk: (text) => {
      shell.feedOutput(text);
      completion.feedOutput(text);
    },
    localFitEnabled: terminalVisible,
    ioEnabled: ownsLivePty,
    renderEnabled: renderTerminal,
    resizeClientId,
    onHistoryReplayStateChange: setHistoryReplaying,
    onConnectionStateChange: setTerminalIoReady,
  });

  useAgentDetection({ termRef, paneId, termReady });

  useTouchScroll({ containerRef, termRef, termReady });

  const touchUi = useTouchOptimizedUi();
  const showTerminalKeyBar = touchUi && uiMode === "terminal" && termReady;
  const fallbackChat = uiMode === "chat" && terminalFallbackOpen;
  const mobileSheet = fallbackChat && touchUi;
  // Clears the git/usage bubbles (top: safe-top + 6px) so the sheet header never overlaps them.
  const SHEET_TOP = "calc(var(--ct-safe-top, 0px) + 2.5rem)";
  const SHEET_HEADER = "2.5rem";
  const SHEET_FOOTER = "3.75rem";
  // reads as a floating overlay above the chat, not an edge-to-edge takeover.
  const FB_TOP = "calc(var(--ct-safe-top, 0px) + 2.75rem)";
  const FB_SIDE = 16;
  const FB_HEADER = "2.5rem";
  const FB_BOTTOM = "calc(5.75rem + var(--ct-composer-bottom-padding, 0.5rem))";
  const FB_SHADOW = "0 24px 64px rgba(0,0,0,0.5)";

  useEffect(() => {
    if (isFocused && ownsLivePty) {
      claimTerminalOwnership();
    }
  }, [isFocused, ownsLivePty, claimTerminalOwnership]);

  useEffect(() => {
    const desired = computeMode();
    setUiMode((mode) => (mode === desired ? mode : desired));
  }, [computeMode]);

  useEffect(() => listenClientPaneUiMode(paneId, (mode) => {
    userModeOverrideRef.current = mode;
    setUiMode(mode === "chat" && !chatCapable ? "terminal" : mode);
  }), [paneId, chatCapable]);

  useEffect(() => listenPaneUiModeDefaultChanged(() => {
    userModeOverrideRef.current = null;
    setUiMode(computeMode());
  }), [computeMode]);

  // Remote-control: an explicit REST /chat-view call (bridge, agents, tests)
  // broadcasts `pane-chat-view`; apply it as if the user toggled locally.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void listen<{ pane_id: string; enabled: boolean }>("pane-chat-view", (event) => {
      if (event.payload.pane_id !== paneId) return;
      setClientPaneUiMode(paneId, event.payload.enabled ? "chat" : "terminal");
    }).then((un) => {
      if (disposed) un();
      else unlisten = un;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [paneId]);

  useEffect(() => {
    publishCurrentPaneUiMode(paneId, uiMode);
  }, [paneId, uiMode]);

  useEffect(() => {
    const handler = (event: Event) => {
      if (!isPaneUiModeEvent(event)) return;
      if (event.detail.paneId !== paneId) return;
      setTerminalFallbackOpen(false);
      const requested = event.detail.mode;
      userModeOverrideRef.current = requested;
      setClientPaneUiMode(paneId, requested);
      if (requested === "chat" && !chatCapable) {
        setUiMode("terminal");
        return;
      }
      setUiMode(requested);
    };
    window.addEventListener("codeterm-pane-ui-mode", handler);
    return () => window.removeEventListener("codeterm-pane-ui-mode", handler);
  }, [paneId, chatCapable]);

  // Tauri file-drop uses the same `term.paste` path as keyboard paste.
  // Bracketed-paste rules stay active when chat is off.
  const { isDragOver: isFileDragOver } = useTauriFileDrop({
    containerRef: paneRootRef,
    enabled: termReady,
    onPaths: (paths) => {
      const term = termRef.current;
      if (!term || paths.length === 0 || uiMode !== "terminal") return;
      term.paste(paths.map(quotePathForPaste).join(" "));
    },
  });

  // Track non-file drag overlay too (browser text drags). Tauri handles files, HTML dragenter handles text.
  const [isTextDragOver, setIsTextDragOver] = useState(false);
  const showDropOverlay = shouldShowDropOverlay({
    isFileDragOver,
    isTextDragOver,
    termReady,
  });

  const { copySelection, pasteFromClipboard } = useTerminalInput({
    termRef, connectedPtyRef, containerRef, termReady,
    terminalActive: terminalVisible,
    onTabKey: completion.handleTab,
    onNonTabKey: completion.dismissCompletions,
    onInputData: completion.feedInput,
  });

  // Refit xterm when the terminal layout changes so rows match the actual terminal box.
  useEffect(() => {
    if (!terminalVisible || !isVisible || !termReady || historyReplaying) return;
    const raf = requestAnimationFrame(() => {
      const term = termRef.current;
      if (!term) return;
      try {
        fitAddonRef.current?.fit();
        commitResize();
        term.refresh(0, term.rows - 1);
      } catch { /* ignore */ }
      termRef.current?.focus();
    });
    return () => cancelAnimationFrame(raf);
  }, [terminalVisible, isVisible, showTerminalKeyBar, termReady, historyReplaying, termRef, fitAddonRef, commitResize]);

  useEffect(() => {
    if (!terminalFallbackOpen || uiMode !== "chat") return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (!target) return;
      if (
        target instanceof Element
        && (target.closest("[data-terminal-fallback-toggle='true']")
          || target.closest("[data-terminal-keys='true']"))
      ) {
        return;
      }
      const shell = fallbackShellRef.current;
      if (shell?.contains(target)) return;
      const terminal = containerRef.current;
      if (terminal?.contains(target)) return;
      setTerminalFallbackOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, [terminalFallbackOpen, uiMode]);

  const handleClick = useCallback(() => {
    onFocus?.(paneId);
    claimTerminalOwnership(true);
    termRef.current?.focus();
  }, [onFocus, paneId, termRef, claimTerminalOwnership]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    listen<{ status: PaneStatus }>(
      `pane-status-${paneId}`,
      (event) => { if (!cancelled) updatePaneStatus(paneId, event.payload.status); },
    ).then((fn) => { if (cancelled) fn(); else unlisten = fn; });
    return () => { cancelled = true; unlisten?.(); };
  }, [paneId, updatePaneStatus]);

  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.paneId === paneId) setSearchOpen((v) => !v);
    };
    window.addEventListener("codeterm-toggle-search", handler);
    return () => window.removeEventListener("codeterm-toggle-search", handler);
  }, [paneId]);

  // Register this pane in global paste registry so app-level Ctrl/Cmd+V always targets focused pane.
  // Direct function lookup avoids event-bus race conditions and remount windows.
  useEffect(() => {
    return registerPanePaste(paneId, () => {
      const term = termRef.current;
      if (term) void routeClipboardToTerm(term);
    });
  }, [paneId, termRef]);

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY });
  }, []);

  const openTerminalFallback = useCallback(() => {
    setTerminalFallbackOpen(true);
    requestAnimationFrame(() => {
      claimTerminalOwnership(true);
      termRef.current?.focus();
    });
  }, [claimTerminalOwnership, termRef]);

  const contextMenuActions: ContextMenuAction[] = [
    { label: "Copy", shortcut: "Ctrl+Shift+C", onClick: copySelection },
    { label: "Paste", shortcut: "Ctrl+Shift+V", onClick: pasteFromClipboard },
  ];

  const topAccent = bellFlash
    ? "var(--ct-yellow, #d29922)"
    : hasSplits && isFocused
      ? "var(--ct-muted-fg, #8b949e)"
      : "transparent";
  const terminalBackground = theme.background;
  const terminalKeyBar = <TerminalKeyBar onSendKey={writePty} className="w-full" />;
  const bootOverlay = paneBootOverlayView(pane);
  const retryAgent = useCallback(() => {
    invoke("restart_agent_pane", { paneId }).catch((e) =>
      console.warn("restart_agent_pane failed:", e),
    );
  }, [paneId]);

  return (
    <div
      ref={paneRootRef}
      className="w-full h-full relative"
      data-testid="terminal-pane"
      data-pane-id={paneId}
      style={{
        boxSizing: "border-box",
        borderTop: hasSplits ? `1px solid ${topAccent}` : "none",
        outline: "none",
        boxShadow: "none",
        background: terminalBackground,
        transition: bellFlash ? "border-color 0.1s ease-in" : "border-color 0.15s ease-out",
      }}
      onMouseDown={handleClick}
      onContextMenu={handleContextMenu}
      onDragEnter={(e) => {
        // Only set overlay for real drags with transferable data.
        if (e.dataTransfer.types.length > 0) setIsTextDragOver(true);
      }}
      onDragOver={(e) => { e.preventDefault(); e.stopPropagation(); }}
      onDragLeave={(e) => {
        // Clear overlay only when the cursor leaves pane bounds.
        if (!e.relatedTarget || !(e.currentTarget as Node).contains(e.relatedTarget as Node)) {
          setIsTextDragOver(false);
        }
      }}
      onDrop={(e) => {
        // Text-only drags go through term.paste to keep bracketed mode.
        e.preventDefault();
        e.stopPropagation();
        setIsTextDragOver(false);
        const term = termRef.current;
        if (!term || uiMode !== "terminal") return;
        const text = e.dataTransfer.getData("text/plain");
        if (text) term.paste(text);
      }}
    >
      {/* Search bar */}
      {searchOpen && (
        <SearchBar
          searchAddon={searchAddonRef.current}
          onClose={() => {
            setSearchOpen(false);
            termRef.current?.focus();
          }}
        />
      )}

      {/* Keep the xterm mount node stable. xterm is opened once into containerRef; remounting that node breaks viewport sizing. */}
      <div
        className={showTerminalKeyBar
          ? "absolute inset-0 grid grid-rows-[minmax(0,1fr)_auto] overflow-hidden"
          : "absolute"
        }
        data-testid="terminal-frame"
        data-terminal-visible={terminalVisible ? "true" : "false"}
        aria-busy={!terminalIoReady}
        style={{
          top: showTerminalKeyBar
            ? undefined
            : mobileSheet
              ? `calc(${SHEET_TOP} + ${SHEET_HEADER})`
              : fallbackChat ? `calc(${FB_TOP} + ${FB_HEADER})` : 0,
          right: showTerminalKeyBar ? undefined : fallbackChat ? (mobileSheet ? 10 : FB_SIDE) : 0,
          bottom: showTerminalKeyBar
            ? undefined
            : mobileSheet
              ? `calc(0.75rem + ${SHEET_FOOTER})`
              : fallbackChat
                ? FB_BOTTOM
                : 0,
          left: showTerminalKeyBar ? undefined : fallbackChat ? (mobileSheet ? 10 : FB_SIDE) : 0,
          height: undefined,
          borderRadius: mobileSheet ? 0 : fallbackChat ? "0 0 14px 14px" : 0,
          border: fallbackChat
            ? "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 26%, transparent)"
            : "none",
          borderTop: mobileSheet ? "none" : fallbackChat ? "none" : undefined,
          borderBottom: mobileSheet ? "none" : undefined,
          boxShadow: fallbackChat && !mobileSheet ? FB_SHADOW : undefined,
          background: terminalBackground,
          visibility: terminalVisible && !historyReplaying ? "visible" : "hidden",
          pointerEvents: terminalVisible ? "auto" : "none",
          zIndex: terminalFallbackOpen ? 4 : uiMode === "terminal" ? 1 : 0,
          animation: mobileSheet
            ? "ct-sheet-in 240ms cubic-bezier(0.32, 0.72, 0, 1)"
            : fallbackChat
              ? "ct-fb-in 200ms cubic-bezier(0.32, 0.72, 0, 1)"
              : undefined,
        }}
      >
        {/* Padding lives on this frame, not on the xterm parent: FitAddon sizes
            rows from the mount node's box and is blind to the parent's own padding. */}
        <div
          className={showTerminalKeyBar
            ? "min-h-0 min-w-0 overflow-hidden"
            : "absolute inset-0 overflow-hidden"
          }
          style={{
            padding: fallbackChat || showTerminalKeyBar ? 8 : 0,
            paddingTop: touchUi && uiMode === "terminal"
              ? "calc(1.25rem + var(--ct-safe-top, 0px))"
              : fallbackChat || showTerminalKeyBar ? 8 : 0,
            opacity: shell.overlayVisible ? 0 : 1,
            transition: "opacity 0.3s ease-in",
            background: terminalBackground,
          }}
          onClick={() => termRef.current?.focus()}
        >
          <div ref={containerRef} className="terminal-xterm-host w-full h-full" data-testid="terminal-xterm-host" />
        </div>
        {showTerminalKeyBar && (
          <div
            className="min-w-0 border-t px-2 pt-2"
            data-testid="terminal-keybar-container"
            style={{
              paddingBottom: "var(--ct-composer-bottom-padding, 0.5rem)",
              borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 24%, transparent)",
              background: terminalBackground,
            }}
          >
            {terminalKeyBar}
          </div>
        )}
      </div>

      {mobileSheet && (
        <div
          className="absolute inset-0"
          data-testid="terminal-sheet-backdrop"
          style={{
            zIndex: 3,
            background: "rgba(0, 0, 0, 0.6)",
            backdropFilter: "blur(2px)",
            WebkitBackdropFilter: "blur(2px)",
            animation: "ct-backdrop-in 160ms ease-out",
          }}
        />
      )}

      <div
        className="absolute inset-0"
        inert={uiMode !== "chat" || !isVisible || undefined}
        style={{
          zIndex: 2,
          visibility: uiMode === "chat" ? "visible" : "hidden",
          pointerEvents: uiMode === "chat" && isVisible ? "auto" : "none",
        }}
      >
        <PaneSessionChat
          paneId={paneId}
          api={api}
          uiFontScale={uiFontScale}
          isVisible={uiMode === "chat" && isVisible}
          onOpenTerminalFallback={openTerminalFallback}
        />
      </div>

      {fallbackChat && !mobileSheet && (
        <div ref={fallbackShellRef} className="absolute inset-0 pointer-events-none" style={{ zIndex: 5 }}>
          <div
            className="absolute flex items-center justify-between px-3 pointer-events-auto"
            style={{
              top: FB_TOP,
              left: FB_SIDE,
              right: FB_SIDE,
              height: FB_HEADER,
              background: "color-mix(in srgb, var(--ct-bg, #0d1117) 92%, black 8%)",
              color: "var(--ct-fg, #c9d1d9)",
              border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 26%, transparent)",
              borderBottom: "none",
              borderRadius: "14px 14px 0 0",
              boxShadow: FB_SHADOW,
              animation: "ct-fb-in 200ms cubic-bezier(0.32, 0.72, 0, 1)",
            }}
            data-testid="terminal-fallback-header"
          >
            <span
              className="flex items-center gap-2 text-[13px] font-semibold tracking-tight"
              style={{ color: "var(--ct-fg)" }}
            >
              <SquareTerminal size={15} style={{ color: "var(--ct-accent, #58a6ff)" }} />
              Terminal
            </span>
            <button
              type="button"
              data-testid="terminal-fallback-close"
              className="inline-flex h-7 w-7 items-center justify-center rounded-lg transition hover:bg-[color-mix(in_srgb,var(--ct-muted-fg,#8b949e)_18%,transparent)]"
              style={{ color: "var(--ct-muted-fg)" }}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => setTerminalFallbackOpen(false)}
              title="Close"
            >
              <X size={15} />
            </button>
          </div>
        </div>
      )}

      {mobileSheet && (
        <div ref={fallbackShellRef} className="absolute inset-0 pointer-events-none" style={{ zIndex: 5 }}>
          {/* Sheet header: grabber, live title, native Done */}
          <div
            className="absolute pointer-events-auto flex items-center justify-between px-3"
            data-testid="terminal-sheet-header"
            style={{
              top: SHEET_TOP,
              left: 10,
              right: 10,
              height: SHEET_HEADER,
              background: "color-mix(in srgb, var(--ct-bg, #0d1117) 92%, black 8%)",
              border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 30%, transparent)",
              borderBottom: "none",
              borderRadius: "14px 14px 0 0",
              boxShadow: "0 -8px 40px rgba(0,0,0,0.5)",
              animation: "ct-sheet-in 240ms cubic-bezier(0.32, 0.72, 0, 1)",
            }}
          >
            <div
              className="absolute left-1/2 -translate-x-1/2 top-1.5 h-1 w-9 rounded-full"
              style={{ background: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 35%, transparent)" }}
            />
            <span className="flex items-center gap-1.5 text-xs font-mono pt-1.5" style={{ color: "var(--ct-fg, #c9d1d9)" }}>
              <span className="h-1.5 w-1.5 rounded-full" style={{ background: "var(--ct-green, #3fb950)" }} />
              terminal
            </span>
            <button
              type="button"
              data-testid="terminal-sheet-done"
              className="mt-1 flex h-8 items-center gap-1 rounded-full px-3 text-[13px] font-semibold select-none touch-manipulation transition-transform active:scale-95"
              style={{
                background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 18%, transparent)",
                border: "1px solid color-mix(in srgb, var(--ct-accent, #58a6ff) 36%, transparent)",
                color: "var(--ct-accent, #58a6ff)",
              }}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => setTerminalFallbackOpen(false)}
            >
              <X size={14} strokeWidth={2.4} />
              Done
            </button>
          </div>
          {/* Sheet footer: integrated key bar */}
          <div
            className="absolute pointer-events-auto px-3 pt-2"
            data-testid="terminal-sheet-footer"
            style={{
              bottom: "0.75rem",
              left: 10,
              right: 10,
              height: SHEET_FOOTER,
              background: "color-mix(in srgb, var(--ct-bg, #0d1117) 92%, black 8%)",
              border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 30%, transparent)",
              borderTop: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 18%, transparent)",
              borderRadius: "0 0 14px 14px",
              boxShadow: "0 8px 40px rgba(0,0,0,0.5)",
              animation: "ct-sheet-in 240ms cubic-bezier(0.32, 0.72, 0, 1)",
            }}
          >
            {terminalKeyBar}
          </div>
        </div>
      )}

      {/* Drop-target overlay — fades in while a file or text drag is
          hovering this pane so the user gets clear "release to paste" feedback.
          pointer-events: none so the drag continues to bubble naturally. */}
      <div
        className="absolute inset-2 rounded-md flex items-center justify-center pointer-events-none"
        data-testid="terminal-drop-overlay"
        style={{
          opacity: showDropOverlay ? 1 : 0,
          transition: "opacity 120ms ease-out",
          background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, transparent)",
          border: "2px dashed color-mix(in srgb, var(--ct-accent, #58a6ff) 70%, transparent)",
          zIndex: 5,
        }}
      >
        <div
          className="px-3 py-1.5 rounded text-[12px] font-medium"
          data-testid="terminal-drop-overlay-label"
          style={{
            background: "color-mix(in srgb, var(--ct-bg, #0d1117) 80%, transparent)",
            color: "var(--ct-fg, #c9d1d9)",
            border: "1px solid color-mix(in srgb, var(--ct-accent, #58a6ff) 50%, transparent)",
          }}
        >
          Drop to paste
        </div>
      </div>

      {/* Loading overlay */}
      <PaneBootOverlay
        view={bootOverlay}
        shellReady={shell.shellReady}
        overlayVisible={shell.overlayVisible}
        onRetry={retryAgent}
      />

      {/* Context menu */}
      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          actions={contextMenuActions}
          onClose={() => setContextMenu(null)}
        />
      )}

      {/* Tab completion popup */}
      {completion.completionItems.length > 0 && (
        <CompletionPopup
          items={completion.completionItems}
          anchorX={completion.completionAnchor.x}
          anchorY={completion.completionAnchor.y}
          onAccept={completion.acceptCompletion}
          onDismiss={completion.dismissCompletions}
        />
      )}
    </div>
  );
});
