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

    // Smart Ctrl+C / Cmd+C: copy if text selected, interrupt if not
    // Leave Ctrl/Cmd+V to the native browser/xterm paste path. The custom
    // clipboard pipeline is still exposed via explicit Paste actions
    // (Ctrl+Shift+V/context menu) for file/image-aware paste into the PTY.
    // Read connectedPtyRef.current dynamically in handlers — PTY ID can change
    // when backend replaces the auto-spawned shell (e.g. General Agent lifecycle).
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown") return true;
      const mod = isMac ? e.metaKey : e.ctrlKey;
      if (!mod || e.shiftKey || e.altKey) return true;
      if (isMac && e.ctrlKey) return true;

      if (e.code === "KeyC") {
        const sel = term.getSelection();
        if (sel) {
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
