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
    navigator.clipboard.readText().then((text) => {
      if (text) invoke("write_pty_string", { ptyId: currentPtyId, data: text });
    }).catch(() => {});
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

    const ptyId = connectedPtyRef.current;

    // Smart Ctrl+C / Cmd+C: copy if text selected, interrupt if not
    // Ctrl+V: platform-specific paste (image + text)
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

      if (e.code === "KeyV") {
        // xterm handles text natively with bracketed paste. We only inject
        // non-text items (images, files, native file refs) alongside.
        pasteClipboardToPty(ptyId, { skipText: true }).catch(() => {});
        return true;
      }

      return true;
    });

    // Primary: xterm.js onData — Tab completion + PTY write
    const onDataDisposable = term.onData((data) => {
      if (data === "\x16") return; // Ctrl+V handled above

      if (data === "\t") {
        if (onTabKey()) return;
      }

      if (data !== "\t") onNonTabKey();

      onInputData(data);
      invoke("write_pty_string", { ptyId, data });
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
