import { useEffect, useRef, useCallback, useState } from "react";
import { listen } from "@tauri-apps/api/event";
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
} from "../../hooks/terminal";
import { quotePathForPaste, routeClipboardToTerm } from "../../lib/clipboardPaste";
import { registerPanePaste } from "../../lib/panePasteRegistry";
import { ContextMenu, type ContextMenuAction } from "./ContextMenu";
import { CompletionPopup } from "./CompletionPopup";
import { SearchBar } from "./SearchBar";
import type { PaneStatus, ThemeConfig, FontConfig, TerminalConfigSettings } from "../../lib/types";

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
  const connectedPtyRef = useRef<string | null>(null);

  const { updatePaneStatus } = useMultiplexer();

  // --- Terminal lifecycle ---
  const { termRef, fitAddonRef, searchAddonRef, bellFlash, termReady } = useXterm({
    containerRef, theme, fontConfig, terminalConfig,
  });

  // --- Resize handling ---
  const { jiggleResize } = useTerminalResize({
    containerRef, termRef, fitAddonRef, connectedPtyRef, termReady, isFocused,
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
  usePtyConnection({
    termRef, fitAddonRef, connectedPtyRef, paneId, ptyId, termReady,
    onPtySpawned,
    onShellReady: shell.markReady,
    onOutputChunk: (text) => {
      shell.feedOutput(text);
      completion.feedOutput(text);
    },
    jiggleResize,
  });

  // --- Agent detection ---
  useAgentDetection({ termRef, paneId, termReady });

  // --- Tauri native drag-drop (provides real file paths, no byte copy) ---
  // Routes through term.paste so xterm wraps with bracketed-paste markers
  // only when the running app has enabled bracketed mode — same single
  // pipeline as keyboard paste.
  const { isDragOver: isFileDragOver } = useTauriFileDrop({
    containerRef,
    enabled: termReady,
    onPaths: (paths) => {
      const term = termRef.current;
      if (!term || paths.length === 0) return;
      term.paste(paths.map(quotePathForPaste).join(" "));
    },
  });

  // Tracks "user is dragging non-file content (text from another app) over
  // this pane". Tauri's drag-drop covers FILE drags, HTML dragenter covers
  // everything else (selected text from a browser, etc.). Combined visually.
  const [isTextDragOver, setIsTextDragOver] = useState(false);
  const showDropOverlay = (isFileDragOver || isTextDragOver) && termReady;

  // --- Input handling ---
  const { copySelection, pasteFromClipboard } = useTerminalInput({
    termRef, connectedPtyRef, containerRef, termReady,
    onTabKey: completion.handleTab,
    onNonTabKey: completion.dismissCompletions,
    onInputData: completion.feedInput,
  });

  // --- UI state ---
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);

  const handleClick = useCallback(() => {
    onFocus?.(paneId);
    termRef.current?.focus();
  }, [onFocus, paneId, termRef]);

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

  // --- App-wide paste routing ---
  // Register this pane's paste fn in the global registry so App.tsx's
  // window-level Ctrl+V handler can target whichever pane is currently
  // focused, regardless of where the inner DOM focus is. Synchronous
  // function-call lookup — no event bus race, no remount window where
  // the dispatch falls into a void.
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
        // Only flip for drags carrying actual draggable data — ignores DOM
        // drag interactions from internal UI (resizing splits, etc.) which
        // don't put anything on `dataTransfer.types`.
        if (e.dataTransfer.types.length > 0) setIsTextDragOver(true);
      }}
      onDragOver={(e) => { e.preventDefault(); e.stopPropagation(); }}
      onDragLeave={(e) => {
        // dragleave fires when moving between children too. Only clear when
        // the cursor truly leaves the pane bounds — relatedTarget is null
        // (or outside `currentTarget`) on a real exit.
        if (!e.relatedTarget || !(e.currentTarget as Node).contains(e.relatedTarget as Node)) {
          setIsTextDragOver(false);
        }
      }}
      onDrop={(e) => {
        // File drops are handled by Tauri's native drag-drop (useTauriFileDrop).
        // This handler covers plain-text drops (drag selection from browser,
        // etc.). Routes through term.paste so the bracketed-paste wrap is
        // governed by xterm's live mode state — same pipeline as every
        // other paste path.
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

      <div
        ref={containerRef}
        className="w-full h-full"
        style={{ padding: 8, opacity: shell.overlayVisible ? 0 : 1, transition: "opacity 0.3s ease-in" }}
        onClick={() => termRef.current?.focus()}
      />

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
