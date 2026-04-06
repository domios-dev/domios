import { useEffect, useRef, useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
} from "../../hooks/terminal";
import { dropFilesToPty } from "../../lib/clipboardPaste";
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

  const { state, updatePaneStatus, closePane } = useMultiplexer();

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

  // --- Input handling ---
  const { copySelection, pasteFromClipboard, clearTerminal } = useTerminalInput({
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

  // --- Screen capture ---
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    listen<{ pane_id: string }>("screen-capture-request", (event) => {
      if (event.payload.pane_id !== paneId) return;
      const term = termRef.current;
      if (!term) return;
      const lines: string[] = [];
      const buf = term.buffer.active;
      for (let i = 0; i < buf.length; i++) {
        const line = buf.getLine(i);
        if (line) lines.push(line.translateToString(true));
      }
      while (lines.length > 0 && lines[lines.length - 1].trim() === "") lines.pop();
      const text = lines.join("\n");
      import("@tauri-apps/api/event").then(({ emit }) => {
        emit(`screen-capture-response-${paneId}`, { text });
      });
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, [paneId, termRef]);

  // --- Search toggle ---
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.paneId === paneId) setSearchOpen((v) => !v);
    };
    window.addEventListener("codeterm-toggle-search", handler);
    return () => window.removeEventListener("codeterm-toggle-search", handler);
  }, [paneId]);

  // --- Context menu ---
  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY });
  }, []);

  const isProtected = state.panes[paneId]?.protected;
  const contextMenuActions: ContextMenuAction[] = [
    { label: "Copy", shortcut: "Ctrl+Shift+C", onClick: copySelection },
    { label: "Paste", shortcut: "Ctrl+Shift+V", onClick: pasteFromClipboard },
    { label: "Clear", onClick: clearTerminal, separator: true },
    ...(!isProtected ? [
      {
        label: "Close Pane",
        shortcut: "Ctrl+Shift+Q",
        onClick: () => closePane(paneId),
        separator: true,
      },
    ] as ContextMenuAction[] : []),
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
      onDragOver={(e) => { e.preventDefault(); e.stopPropagation(); }}
      onDrop={async (e) => {
        e.preventDefault();
        e.stopPropagation();
        const currentPtyId = connectedPtyRef.current;
        if (!currentPtyId) return;

        const files = Array.from(e.dataTransfer.files || []);
        if (files.length > 0) {
          // All files (image or not) → tmp → path (Telegram-style)
          await dropFilesToPty(currentPtyId, files);
          return;
        }

        // Plain text drop → bracket paste
        const text = e.dataTransfer.getData("text/plain");
        if (text) {
          const bracketedPaste = `\x1b[200~${text}\x1b[201~`;
          invoke("write_pty_string", { ptyId: currentPtyId, data: bracketedPaste });
        }
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
