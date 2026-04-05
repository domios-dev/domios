import { useEffect, useCallback } from "react";
import type { Terminal } from "@xterm/xterm";
import { invoke } from "@tauri-apps/api/core";
import { isMac } from "../../lib/keybindings";

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
        if (isMac) {
          navigator.clipboard.read().then(async (items) => {
            for (const item of items) {
              const imageType = item.types.find((t: string) => t.startsWith("image/"));
              if (imageType) {
                const blob = await item.getType(imageType);
                const ext = imageType === "image/jpeg" ? "jpg" : "png";
                const arrayBuf = await blob.arrayBuffer();
                const bytes = Array.from(new Uint8Array(arrayBuf));
                const path = await invoke<string>("save_clipboard_image", { bytes, ext });
                if (path) invoke("write_pty_string", { ptyId, data: path });
              }
            }
          }).catch(() => {});
          return true;
        } else {
          e.preventDefault();
          navigator.clipboard.read().then(async (items) => {
            const textItem = items.find((item) => item.types.includes("text/plain"));
            if (textItem) {
              const blob = await textItem.getType("text/plain");
              const text = await blob.text();
              if (text) {
                invoke("write_pty_string", { ptyId, data: text });
                return;
              }
            }
            for (const item of items) {
              const imageType = item.types.find((t: string) => t.startsWith("image/"));
              if (imageType) {
                const blob = await item.getType(imageType);
                const ext = imageType === "image/jpeg" ? "jpg" : "png";
                const arrayBuf = await blob.arrayBuffer();
                const bytes = Array.from(new Uint8Array(arrayBuf));
                const path = await invoke<string>("save_clipboard_image", { bytes, ext });
                if (path) invoke("write_pty_string", { ptyId, data: path });
                return;
              }
            }
          }).catch(() => {
            navigator.clipboard.readText().then((text) => {
              if (text) invoke("write_pty_string", { ptyId, data: text });
            }).catch(() => {});
          });
          return false;
        }
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

    // Focus xterm on any click in the container
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
