import { useEffect, useRef, useCallback, useMemo, useState } from "react";
import { invoke } from "../../lib/bridge";
import { IS_WEB, listen } from "../../lib/bridge";
import { useMultiplexer } from "../../store/multiplexer";
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
  getClientPaneUiMode,
  listenClientPaneUiMode,
  setClientPaneUiMode,
} from "../../lib/clientPaneUiMode";
import { ContextMenu, type ContextMenuAction } from "./ContextMenu";
import { TerminalKeyBar } from "./TerminalKeyBar";
import { CompletionPopup } from "./CompletionPopup";
import { SearchBar } from "./SearchBar";
import { PaneSessionChat } from "./PaneSessionChat";
import {
  isChatUiSupported,
  isPaneUiModeEvent,
  shouldDefaultNewAgentPaneToChat,
  type PaneUiMode,
} from "../../lib/chatUiMode";
import type { PaneStatus, ThemeConfig, FontConfig, TerminalConfigSettings } from "../../lib/types";
import { claimResizeOwner, getResizeClientId } from "../../lib/resizeOwner";

interface TerminalPaneProps {
  paneId: string;
  ptyId?: string | null;
  isFocused?: boolean;
  hasSplits?: boolean;
  onFocus?: (paneId: string) => void;
  onPtySpawned?: (paneId: string, ptyId: string) => void;
  theme?: ThemeConfig;
  fontConfig?: FontConfig;
  terminalConfig?: TerminalConfigSettings;
}

export function TerminalPane({
  paneId,
  ptyId,
  isFocused = false,
  hasSplits = false,
  onFocus,
  onPtySpawned,
  theme,
  fontConfig,
  terminalConfig,
}: TerminalPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const fallbackShellRef = useRef<HTMLDivElement>(null);
  const connectedPtyRef = useRef<string | null>(null);

  const { state, updatePaneStatus } = useMultiplexer();
  const pane = state.panes[paneId];
  const agentType = pane?.agent_type;
  const chatSupported = isChatUiSupported(agentType);
  const sessionReadyForChat = Boolean(pane?.agent_session_id);
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
  const shouldDefaultToChat =
    shouldDefaultNewAgentPaneToChat(agentType) ||
    (mobileWebDefaultChat && chatSupported);

  // --- Terminal lifecycle ---
  const { termRef, fitAddonRef, searchAddonRef, bellFlash, termReady } = useXterm({
    containerRef, theme, fontConfig, terminalConfig,
  });

  // --- UI state ---
  const [uiMode, setUiMode] = useState<PaneUiMode>(() => (
    chatSupported
      && sessionReadyForChat
      && (getClientPaneUiMode(paneId) === "chat" || shouldDefaultToChat)
      ? "chat"
      : "terminal"
  ));
  const userModeOverrideRef = useRef<PaneUiMode | null>(null);
  const [terminalFallbackOpen, setTerminalFallbackOpen] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const terminalVisible = uiMode === "terminal" || terminalFallbackOpen;
  const terminalOwnsLivePty = !IS_WEB || terminalVisible;
  const resizeClientId = useMemo(() => getResizeClientId(), []);
  const claimTerminalOwnership = useCallback(() => {
    if (!terminalOwnsLivePty) return;
    const term = termRef.current;
    const ptyIdNow = connectedPtyRef.current ?? ptyId ?? null;
    if (!term || !ptyIdNow || term.cols <= 0 || term.rows <= 0) return;
    void claimResizeOwner(paneId, ptyIdNow, resizeClientId, term.cols, term.rows)
      .then(() => invoke("resize_pty", {
        ptyId: ptyIdNow,
        cols: term.cols,
        rows: term.rows,
        clientId: resizeClientId,
      }))
      .catch(() => {});
  }, [paneId, ptyId, resizeClientId, terminalOwnsLivePty, termRef, connectedPtyRef]);

  // --- Resize handling ---
  const { jiggleResize } = useTerminalResize({
    containerRef,
    termRef,
    fitAddonRef,
    connectedPtyRef,
    termReady,
    isFocused,
    resizeEnabled: terminalVisible,
    resizeClientId,
  });

  // --- Shell integration (readiness, CWD, pending commands) ---
  const shell = useShellIntegration({
    termRef, connectedPtyRef, paneId, ptyId, termReady,
  });

  // --- Tab completion ---
  const completion = useTabCompletion({
    termRef, containerRef, connectedPtyRef,
  });

  // --- PTY connection ---
  const { writePty } = usePtyConnection({
    termRef, fitAddonRef, connectedPtyRef, paneId, ptyId, termReady,
    onPtySpawned,
    onShellReady: shell.markReady,
    onOutputChunk: (text) => {
      shell.feedOutput(text);
      completion.feedOutput(text);
    },
    jiggleResize,
    localFitEnabled: terminalVisible,
    ioEnabled: terminalOwnsLivePty,
    resizeClientId,
  });

  // --- Agent detection ---
  useAgentDetection({ termRef, paneId, termReady });

  // --- Touch swipe scrolling (xterm has no native touch scroll) ---
  useTouchScroll({ containerRef, termRef, termReady });

  const touchUi = useTouchOptimizedUi();
  const showTerminalKeyBar = touchUi && uiMode === "terminal" && termReady;
  const fallbackChat = uiMode === "chat" && terminalFallbackOpen;
  const mobileSheet = fallbackChat && touchUi;
  const SHEET_TOP = "calc(env(safe-area-inset-top, 0px) + 0.75rem)";
  const SHEET_HEADER = "2.5rem";
  const SHEET_FOOTER = "calc(4.25rem + env(safe-area-inset-bottom, 0px))";

  useEffect(() => {
    if (uiMode !== "chat") {
      void invoke("chat_stop", { paneId }).catch(() => {});
    }
  }, [paneId, uiMode]);

  useEffect(() => {
    if (isFocused && terminalOwnsLivePty) {
      claimTerminalOwnership();
    }
  }, [isFocused, terminalOwnsLivePty, claimTerminalOwnership]);

  useEffect(() => {
    const defaultMode: PaneUiMode = chatSupported
      && sessionReadyForChat
      && ((getClientPaneUiMode(paneId) ?? (shouldDefaultToChat ? "chat" : "terminal")) === "chat")
      ? "chat"
      : "terminal";
    const desiredMode = userModeOverrideRef.current ?? defaultMode;
    setUiMode((mode) => mode === desiredMode ? mode : desiredMode);
  }, [agentType, chatSupported, sessionReadyForChat, paneId, shouldDefaultToChat]);

  useEffect(() => listenClientPaneUiMode(paneId, (mode) => {
    userModeOverrideRef.current = mode;
    setUiMode(mode === "chat" && (!chatSupported || !sessionReadyForChat) ? "terminal" : mode);
  }), [paneId, chatSupported, sessionReadyForChat]);

  useEffect(() => {
    setClientPaneUiMode(paneId, uiMode);
  }, [paneId, uiMode]);

  useEffect(() => {
    const handler = (event: Event) => {
      if (!isPaneUiModeEvent(event)) return;
      if (event.detail.paneId !== paneId) return;
      setTerminalFallbackOpen(false);
      const requested = event.detail.mode;
      userModeOverrideRef.current = requested;
      setClientPaneUiMode(paneId, requested);
      if (requested === "chat" && (!chatSupported || !sessionReadyForChat)) {
        setUiMode("terminal");
        return;
      }
      setUiMode(requested);
    };
    window.addEventListener("codeterm-pane-ui-mode", handler);
    return () => window.removeEventListener("codeterm-pane-ui-mode", handler);
  }, [paneId, chatSupported, sessionReadyForChat]);

  // Tauri file-drop uses the same `term.paste` path as keyboard paste.
  // Bracketed-paste rules stay active when chat is off.
  const { isDragOver: isFileDragOver } = useTauriFileDrop({
    containerRef,
    enabled: termReady && uiMode === "terminal",
    onPaths: (paths) => {
      const term = termRef.current;
      if (!term || paths.length === 0) return;
      term.paste(paths.map(quotePathForPaste).join(" "));
    },
  });

  // Track non-file drag overlay too (browser text drags). Tauri handles files, HTML dragenter handles text.
  const [isTextDragOver, setIsTextDragOver] = useState(false);
  const showDropOverlay = (isFileDragOver || isTextDragOver) && termReady && uiMode === "terminal";

  // --- Input handling ---
  const { copySelection, pasteFromClipboard } = useTerminalInput({
    termRef, connectedPtyRef, containerRef, termReady,
    onTabKey: completion.handleTab,
    onNonTabKey: completion.dismissCompletions,
    onInputData: completion.feedInput,
  });

  // On chat→terminal return, refit xterm once so layout and PTY size stay synchronized.
  useEffect(() => {
    if (uiMode !== "terminal" || !termReady) return;
    const raf = requestAnimationFrame(() => {
      const term = termRef.current;
      const fit = fitAddonRef.current;
      if (!term || !fit) return;
      try {
        fit.fit();
        term.refresh(0, term.rows - 1);
        const ptyId = connectedPtyRef.current;
        if (ptyId && term.cols > 0 && term.rows > 0) {
          void claimResizeOwner(paneId, ptyId, resizeClientId, term.cols, term.rows)
            .then(() => invoke("resize_pty", {
              ptyId,
              cols: term.cols,
              rows: term.rows,
              clientId: resizeClientId,
            }))
            .catch(() => {});
        }
      } catch { /* ignore */ }
      termRef.current?.focus();
    });
    return () => cancelAnimationFrame(raf);
  }, [uiMode, termReady, paneId, resizeClientId, termRef, fitAddonRef, connectedPtyRef]);

  useEffect(() => {
    if (!terminalFallbackOpen || !termReady) return;
    const raf = requestAnimationFrame(() => {
      const term = termRef.current;
      const fit = fitAddonRef.current;
      if (!term || !fit) return;
      try {
        fit.fit();
        term.refresh(0, term.rows - 1);
        const ptyId = connectedPtyRef.current;
        if (ptyId && term.cols > 0 && term.rows > 0) {
          void claimResizeOwner(paneId, ptyId, resizeClientId, term.cols, term.rows)
            .then(() => invoke("resize_pty", {
              ptyId,
              cols: term.cols,
              rows: term.rows,
              clientId: resizeClientId,
            }))
            .catch(() => {});
        }
      } catch { /* ignore */ }
      term.focus();
    });
    return () => cancelAnimationFrame(raf);
  }, [terminalFallbackOpen, termReady, paneId, resizeClientId, termRef, fitAddonRef, connectedPtyRef]);

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
    claimTerminalOwnership();
    termRef.current?.focus();
  }, [onFocus, paneId, termRef, claimTerminalOwnership]);

  // --- Pane status events ---
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    listen<{ status: PaneStatus }>(
      `pane-status-${paneId}`,
      (event) => { if (!cancelled) updatePaneStatus(paneId, event.payload.status); },
    ).then((fn) => { if (cancelled) fn(); else unlisten = fn; });
    return () => { cancelled = true; unlisten?.(); };
  }, [paneId, updatePaneStatus]);

  // --- Search toggle ---
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

  // --- Context menu ---
  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY });
  }, []);

  const contextMenuActions: ContextMenuAction[] = [
    { label: "Copy", shortcut: "Ctrl+Shift+C", onClick: copySelection },
    { label: "Paste", shortcut: "Ctrl+Shift+V", onClick: pasteFromClipboard },
  ];

  // --- Style ---
  const topAccent = bellFlash
    ? "var(--ct-yellow, #d29922)"
    : hasSplits && isFocused
      ? "var(--ct-muted-fg, #8b949e)"
      : "transparent";

  // --- Render ---
  return (
    <div
      className="w-full h-full relative"
      data-pane-id={paneId}
      style={{
        boxSizing: "border-box",
        borderTop: hasSplits ? `1px solid ${topAccent}` : "none",
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
        if (!term) return;
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

      {/* Keep terminal mounted full size + visibility hidden to keep FitAddon dimensions valid on return. */}
      <div
        ref={containerRef}
        className="absolute inset-0"
        style={{
          padding: 8,
          opacity: shell.overlayVisible ? 0 : 1,
          transition: "opacity 0.3s ease-in",
          top: mobileSheet
            ? `calc(${SHEET_TOP} + ${SHEET_HEADER})`
            : fallbackChat ? "auto" : 0,
          right: fallbackChat ? (mobileSheet ? 10 : 12) : 0,
          bottom: mobileSheet
            ? `calc(0.75rem + ${SHEET_FOOTER})`
            : fallbackChat
              ? "calc(5.75rem + var(--ct-composer-bottom-padding, 0.5rem))"
              : showTerminalKeyBar ? "3.5rem" : 0,
          left: fallbackChat ? (mobileSheet ? 10 : 12) : 0,
          height: fallbackChat && !mobileSheet ? "min(38dvh, 320px)" : "auto",
          borderRadius: mobileSheet ? 0 : fallbackChat ? 6 : 0,
          border: fallbackChat
            ? "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 30%, transparent)"
            : "none",
          borderTop: mobileSheet ? "none" : undefined,
          borderBottom: mobileSheet ? "none" : undefined,
          background: "var(--ct-bg, #0d1117)",
          visibility: terminalVisible ? "visible" : "hidden",
          pointerEvents: terminalVisible ? "auto" : "none",
          zIndex: terminalFallbackOpen ? 4 : uiMode === "terminal" ? 1 : 0,
          animation: mobileSheet ? "ct-sheet-in 240ms cubic-bezier(0.32, 0.72, 0, 1)" : undefined,
        }}
        onClick={() => termRef.current?.focus()}
      />
      {showTerminalKeyBar && (
        <div
          className="absolute left-2 right-2"
          style={{ bottom: "calc(0.5rem + var(--ct-composer-bottom-padding, 0rem))", zIndex: 3 }}
        >
          <TerminalKeyBar onSendKey={writePty} />
        </div>
      )}

      {mobileSheet && (
        <div
          className="absolute inset-0"
          style={{
            zIndex: 3,
            background: "rgba(0, 0, 0, 0.6)",
            backdropFilter: "blur(2px)",
            WebkitBackdropFilter: "blur(2px)",
            animation: "ct-backdrop-in 160ms ease-out",
          }}
        />
      )}

      {/* Chat is conditionally mounted so the per-pane tailer thread on
          the backend only starts when the user actually flipped into
          chat mode — not for every pane in the workspace. */}
      {uiMode === "chat" && (
        <div className="absolute inset-0" style={{ zIndex: 2 }}>
          <PaneSessionChat
            paneId={paneId}
            onError={() => {
              setClientPaneUiMode(paneId, "terminal");
              setUiMode("terminal");
            }}
            onOpenTerminalFallback={() => {
              setTerminalFallbackOpen(true);
              requestAnimationFrame(() => termRef.current?.focus());
            }}
            onToggleTerminalFallback={() => {
              setTerminalFallbackOpen((open) => {
                const next = !open;
                if (next) requestAnimationFrame(() => termRef.current?.focus());
                return next;
              });
            }}
          />
        </div>
      )}

      {fallbackChat && !mobileSheet && (
        <div ref={fallbackShellRef} className="absolute inset-0 pointer-events-none" style={{ zIndex: 5 }}>
          <div
            className="absolute left-3 right-3 h-7 flex items-center justify-between px-2 rounded-t-md pointer-events-auto"
            style={{
              bottom:
                "calc(5.75rem + var(--ct-composer-bottom-padding, 0.5rem) + min(38dvh, 320px))",
              background: "color-mix(in srgb, var(--ct-bg, #0d1117) 92%, black 8%)",
              color: "var(--ct-fg, #c9d1d9)",
              border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 30%, transparent)",
              borderBottom: "none",
            }}
          >
            <span className="text-xs font-mono opacity-80">Terminal fallback</span>
            <button
              type="button"
              className="px-2 h-5 rounded text-xs font-mono hover:brightness-125"
              style={{
                background: "var(--ct-surface)",
                color: "var(--ct-muted-fg)",
                border: "none",
              }}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => setTerminalFallbackOpen(false)}
            >
              Close
            </button>
          </div>
        </div>
      )}

      {mobileSheet && (
        <div ref={fallbackShellRef} className="absolute inset-0 pointer-events-none" style={{ zIndex: 5 }}>
          {/* Sheet header: grabber, live title, native Done */}
          <div
            className="absolute pointer-events-auto flex items-center justify-between px-3"
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
              className="h-10 px-2 -mr-2 pt-1.5 text-sm font-medium select-none touch-manipulation active:opacity-60"
              style={{ background: "transparent", border: "none", color: "var(--ct-accent, #58a6ff)" }}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => setTerminalFallbackOpen(false)}
            >
              Done
            </button>
          </div>
          {/* Sheet footer: integrated key bar */}
          <div
            className="absolute pointer-events-auto px-3 pt-2.5"
            style={{
              bottom: "0.75rem",
              left: 10,
              right: 10,
              height: SHEET_FOOTER,
              paddingBottom: "calc(0.625rem + env(safe-area-inset-bottom, 0px))",
              background: "color-mix(in srgb, var(--ct-bg, #0d1117) 92%, black 8%)",
              border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 30%, transparent)",
              borderTop: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 18%, transparent)",
              borderRadius: "0 0 14px 14px",
              boxShadow: "0 8px 40px rgba(0,0,0,0.5)",
              animation: "ct-sheet-in 240ms cubic-bezier(0.32, 0.72, 0, 1)",
            }}
          >
            <TerminalKeyBar onSendKey={writePty} />
          </div>
        </div>
      )}

      {/* Drop-target overlay — fades in while a file or text drag is
          hovering this pane so the user gets clear "release to paste" feedback.
          pointer-events: none so the drag continues to bubble naturally. */}
      <div
        className="absolute inset-2 rounded-md flex items-center justify-center pointer-events-none"
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
      {shell.overlayVisible && (
        <div
          className={`absolute inset-0 ${!shell.shellReady ? "terminal-loading-overlay" : ""}`}
          style={{
            background: "var(--ct-bg, #0d1117)",
            opacity: shell.shellReady ? 0 : 1,
            transition: "opacity 0.3s ease-out",
          }}
        >
          {!shell.shellReady && (
            <div className="absolute bottom-4 left-1/2 -translate-x-1/2">
              <div className="terminal-breathing-bar" />
            </div>
          )}
        </div>
      )}

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
}
