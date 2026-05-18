import { useEffect, useCallback } from "react";
import type { Terminal } from "@xterm/xterm";
import { invoke } from "@tauri-apps/api/core";
import { isMac } from "../../lib/keybindings";
import { pasteClipboardToPty } from "../../lib/clipboardPaste";

export interface UseTerminalInputOptions {
  termRef: React.MutableRefObject<Terminal | null>;
  connectedPtyRef: React.MutableRefObject<string | null>;
  containerRef: React.RefObject<HTMLDivElement | null>;
  termReady: boolean;
  onTabKey: () => boolean;
  onNonTabKey: () => void;
  onInputData: (data: string) => void;
}

export interface UseTerminalInputReturn {
  copySelection: () => void;
  pasteFromClipboard: () => void;
  clearTerminal: () => void;
}

export function useTerminalInput({
  termRef,
  connectedPtyRef,
  containerRef,
  termReady,
  onTabKey,
  onNonTabKey,
  onInputData,
}: UseTerminalInputOptions): UseTerminalInputReturn {
  const copySelection = useCallback(() => {
    const selection = termRef.current?.getSelection();
    if (selection) navigator.clipboard.writeText(selection).catch(() => {});
  }, [termRef]);

  const pasteFromClipboard = useCallback(() => {
    const currentPtyId = connectedPtyRef.current;
    if (!currentPtyId) return;
    pasteClipboardToPty(currentPtyId).catch(() => {});
  }, [connectedPtyRef]);

  const clearTerminal = useCallback(() => {
    termRef.current?.clear();
  }, [termRef]);

  // Register input handlers when PTY is connected
  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady) return;
    // Wait for PTY connection
    if (!connectedPtyRef.current) return;

    // Issue #102: route Ctrl+V / Cmd+V (and Ctrl+Shift+V) through the
    // canonical pasteClipboardToPty handler. Previously this was delegated
    // to "the native browser/xterm paste path" — but xterm.js doesn't
    // wire browser paste in Tauri's WebView, so Ctrl+V silently did nothing.
    // Read connectedPtyRef.current dynamically in handlers — PTY ID can change
    // when backend replaces the auto-spawned shell (e.g. General Agent lifecycle).
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown") return true;
      const mod = isMac ? e.metaKey : e.ctrlKey;
      if (!mod || e.altKey) return true;
      if (isMac && e.ctrlKey && e.metaKey) return true;

      // Paste — Ctrl+V, Cmd+V, Ctrl+Shift+V all funnel through the same
      // file/image/path-aware pipeline. preventDefault + stopPropagation
      // are mandatory: without them WebView2 still fires the native `paste`
      // event on the textarea (xterm.js handles it via its own listener and
      // emits a SECOND bracketed-paste through onData → double paste), and
      // the window-level keydown listener in App.tsx (kb.paste) would also
      // fire and write the clipboard a third time.
      if (e.code === "KeyV") {
        e.preventDefault();
        e.stopPropagation();
        const ptyId = connectedPtyRef.current;
        if (ptyId) {
          pasteClipboardToPty(ptyId).catch(() => {});
        }
        return false;
      }

      // Shift modifier was already rejected for Copy in the original
      // implementation; preserve that — Ctrl+Shift+C was never wired here.
      if (e.shiftKey) return true;

      if (e.code === "KeyC") {
        const sel = term.getSelection();
        if (sel) {
          e.preventDefault();
          e.stopPropagation();
          navigator.clipboard.writeText(sel).catch(() => {});
          term.clearSelection();
          return false;
        }
        return true;
      }

      return true;
    });

    // Primary: xterm.js onData — Tab completion + PTY write
    // Read connectedPtyRef.current dynamically — the PTY ID can change when
    // backend links a different PTY (e.g. General Agent replacing auto-spawned shell).
    const onDataDisposable = term.onData((data) => {
      if (data === "\t") {
        if (onTabKey()) return;
      }

      if (data !== "\t") onNonTabKey();

      onInputData(data);
      const currentPty = connectedPtyRef.current;
      if (currentPty) invoke("write_pty_string", { ptyId: currentPty, data });
    });

    const el = containerRef.current;
    const focusXterm = () => term.focus();
    el?.addEventListener("mousedown", focusXterm);

    return () => {
      onDataDisposable.dispose();
      el?.removeEventListener("mousedown", focusXterm);
    };
  }, [termReady, termRef, connectedPtyRef, containerRef, onTabKey, onNonTabKey, onInputData]);

  return { copySelection, pasteFromClipboard, clearTerminal };
}
