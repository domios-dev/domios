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

  // Native Tauri drop gives real file paths and uses the same `term.paste` path as keyboard paste.
  // Bracketed-paste behavior is still respected when chat is not active on this pane.
  const { isDragOver: isFileDragOver } = useTauriFileDrop({
    containerRef,
    enabled: termReady && uiMode === "terminal",
    onPaths: (paths) => {
      const term = termRef.current;
      if (!term || paths.length === 0) return;
      term.paste(paths.map(quotePathForPaste).join(" "));
    },
  });

  // Tracks "user is dragging non-file content (text from another app) over this pane". Tauri's drag-drop covers FILE drags, HTML dragenter
  // covers everything else (selected text from a browser, etc.). Combined visually.
  const [isTextDragOver, setIsTextDragOver] = useState(false);
  const showDropOverlay = (isFileDragOver || isTextDragOver) && termReady && uiMode === "terminal";

  // --- Input handling ---
  const { copySelection, pasteFromClipboard } = useTerminalInput({
    termRef, connectedPtyRef, containerRef, termReady,
    onTabKey: completion.handleTab,
    onNonTabKey: completion.dismissCompletions,
    onInputData: completion.feedInput,
  });

  // On return from chat mode, re-fit xterm once so stale off-screen layout does not persist.
  // This keeps terminal size/PTY in sync during visibility transitions.
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
        // Only flip for drags carrying actual draggable data — ignores DOM drag interactions from internal UI (resizing splits, etc.) which don't put anything on `dataTransfer.types`.
        if (e.dataTransfer.types.length > 0) setIsTextDragOver(true);
      }}
      onDragOver={(e) => { e.preventDefault(); e.stopPropagation(); }}
      onDragLeave={(e) => {
        // dragleave fires when moving between children too. Only clear when the cursor truly leaves the pane bounds — relatedTarget is null (or outside `currentTarget`) on a real exit.
        if (!e.relatedTarget || !(e.currentTarget as Node).contains(e.relatedTarget as Node)) {
          setIsTextDragOver(false);
        }
      }}
      onDrop={(e) => {
        // File drops are handled by Tauri's native drag-drop (useTauriFileDrop). This handler covers plain-text drops (drag selection from browser,
        // etc.). Routes through term.paste so the bracketed-paste wrap is governed by xterm's live mode state — same pipeline as every other paste path.
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

      {/* xterm container — always mounted at full size so the FitAddon
          never sees a 0×0 box (display:none triggers ResizeObserver with
          clientWidth=0 → fit() reshapes to 1 column → garbled buffer on
          return). visibility:hidden preserves layout dimensions so xterm
          stays correctly sized while the chat view is on top. */}
      <div
        ref={containerRef}
        className="absolute inset-0"
        style={{
          padding: 8,
          opacity: shell.overlayVisible ? 0 : 1,
          transition: "opacity 0.3s ease-in",
          top: uiMode === "chat" && terminalFallbackOpen ? "auto" : 0,
          right: uiMode === "chat" && terminalFallbackOpen ? 12 : 0,
          bottom: uiMode === "chat" && terminalFallbackOpen
            ? "calc(5.75rem + var(--ct-composer-bottom-padding, 0.5rem))"
            : showTerminalKeyBar ? "3.5rem" : 0,
          left: uiMode === "chat" && terminalFallbackOpen ? 12 : 0,
          height: uiMode === "chat" && terminalFallbackOpen
            ? "min(38dvh, 320px)"
            : "auto",
          borderRadius: uiMode === "chat" && terminalFallbackOpen ? 6 : 0,
          border: uiMode === "chat" && terminalFallbackOpen
            ? "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 30%, transparent)"
            : "none",
          background: "var(--ct-bg, #0d1117)",
          visibility: terminalVisible ? "visible" : "hidden",
          pointerEvents: terminalVisible ? "auto" : "none",
          zIndex: terminalFallbackOpen ? 4 : uiMode === "terminal" ? 1 : 0,
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

      {/* Chat is conditionally mounted so the per-pane tailer thread on
          the backend only starts when the user actually flipped into
          chat mode — not for every pane in the workspace. */}
      {uiMode === "chat" && (
        <div className="absolute inset-0" style={{ zIndex: 2 }}>
          <PaneSessionChat
            paneId={paneId}
            terminalFallbackOpen={terminalFallbackOpen}
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

      {uiMode === "chat" && terminalFallbackOpen && (
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
