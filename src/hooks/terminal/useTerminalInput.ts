import { useEffect, useCallback } from "react";
import type { Terminal } from "@xterm/xterm";
import { invoke } from "@tauri-apps/api/core";
import { isMac } from "../../lib/keybindings";
import {
  extractFilePathsFromClipboard,
  quotePathForPaste,
} from "../../lib/clipboardPaste";

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

// Single paste point. Uses xterm's `term.paste(text)` public API so the
// transformation (line-ending normalization, bracketed-paste-mode wrapping
// based on the terminal's actual state, transmission through onData) is
// done by the library itself — no hand-rolled `\x1b[200~` we got wrong
// before. If file paths are present on the clipboard, send them as
// shell-quoted strings.
async function readClipboardFiles(): Promise<string[]> {
  try {
    const paths = await invoke<string[]>("read_clipboard_files");
    return paths || [];
  } catch {
    return [];
  }
}

function pasteTextOrPaths(term: Terminal, text: string | null, paths: string[]): void {
  if (paths.length > 0) {
    term.paste(paths.map(quotePathForPaste).join(" "));
    return;
  }
  if (text) term.paste(text);
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

  // Programmatic paste — called from the custom right-click menu and the
  // Ctrl+Shift+V window-level handler. Asynchronously reads the system
  // clipboard (no DOM paste event in this path) and routes through
  // term.paste so bracketed-paste-mode wrapping happens correctly.
  const pasteFromClipboard = useCallback(() => {
    const term = termRef.current;
    if (!term) return;
    (async () => {
      const paths = await readClipboardFiles();
      if (paths.length > 0) {
        term.paste(paths.map(quotePathForPaste).join(" "));
        return;
      }
      try {
        const text = await navigator.clipboard.readText();
        if (text) term.paste(text);
      } catch {
        /* clipboard read denied — nothing to paste */
      }
    })();
  }, [termRef]);

  const clearTerminal = useCallback(() => {
    termRef.current?.clear();
  }, [termRef]);

  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady) return;
    if (!connectedPtyRef.current) return;

    // Copy on Ctrl+C/Cmd+C ONLY when there's a selection. Without a
    // selection we let xterm send SIGINT through as usual. Paste is NOT
    // handled here — see the `paste` event listener below.
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown") return true;
      const mod = isMac ? e.metaKey : e.ctrlKey;
      if (!mod || e.altKey || e.shiftKey) return true;
      if (isMac && e.ctrlKey && e.metaKey) return true;

      if (e.code === "KeyC") {
        const sel = term.getSelection();
        if (sel) {
          navigator.clipboard.writeText(sel).catch(() => {});
          term.clearSelection();
          return false;
        }
      }
      return true;
    });

    const onDataDisposable = term.onData((data) => {
      if (data === "\t" && onTabKey()) return;
      if (data !== "\t") onNonTabKey();
      onInputData(data);
      const currentPty = connectedPtyRef.current;
      if (currentPty) invoke("write_pty_string", { ptyId: currentPty, data });
    });

    // SINGLE PASTE PATH (the lesson from a long evening of debugging).
    //
    // xterm.js registers its own `paste` listener on the textarea inside
    // the container and exposes no public API to remove it (xtermjs/xterm.js
    // #1803, #3516). Without intervention BOTH xterm's listener AND any
    // custom Ctrl+V keydown handler we add fire on every paste, producing
    // the double-paste seen on some machines and on others the wrong
    // bracketed-mode envelope (raw `\x1b[200~` written when bracketed mode
    // was actually OFF, looking like the paste did nothing).
    //
    // The fix is a capture-phase listener on the container. Capture phase
    // fires BEFORE the textarea's bubble-phase listener (where xterm
    // attaches its handler), so stopImmediatePropagation here cancels
    // xterm's listener entirely. We then call `term.paste(text)` — the
    // public xterm API that respects the live bracketed-paste-mode state,
    // normalises line endings, and emits via onData (which our onData
    // handler above writes to the PTY). One call, one paste, always
    // correct envelope.
    //
    // Programmatic pastes (custom right-click menu, drag-drop) bypass
    // this entirely and call term.paste directly — they don't trigger a
    // DOM paste event.
    const el = containerRef.current;
    const onPaste = (ev: Event) => {
      const e = ev as ClipboardEvent;
      const cd = e.clipboardData;
      if (!cd) return;
      ev.stopImmediatePropagation();
      ev.preventDefault();

      const paths = extractFilePathsFromClipboard(cd);
      if (paths.length > 0) {
        // Resolve any virtual file refs the OS hides behind text/uri-list.
        readClipboardFiles().then((nativePaths) => {
          const final = nativePaths.length > 0 ? nativePaths : paths;
          pasteTextOrPaths(term, cd.getData("text/plain") || null, final);
        });
        return;
      }
      const text = cd.getData("text/plain");
      if (text) term.paste(text);
    };
    el?.addEventListener("paste", onPaste, true);

    const focusXterm = () => term.focus();
    el?.addEventListener("mousedown", focusXterm);

    return () => {
      onDataDisposable.dispose();
      el?.removeEventListener("mousedown", focusXterm);
      el?.removeEventListener("paste", onPaste, true);
    };
  }, [termReady, termRef, connectedPtyRef, containerRef, onTabKey, onNonTabKey, onInputData]);

  return { copySelection, pasteFromClipboard, clearTerminal };
}
