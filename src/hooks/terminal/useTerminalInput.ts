import { useEffect, useCallback } from "react";
import type { Terminal } from "@xterm/xterm";
import { invoke } from "../../lib/bridge";
import { isMac } from "../../lib/keybindings";
import { routeClipboardToTerm } from "../../lib/clipboardPaste";

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
  // Ctrl+Shift+V window-level handler. Funnels through the same unified
  // route as the keyboard path so file/image/text handling is consistent.
  const pasteFromClipboard = useCallback(() => {
    const term = termRef.current;
    if (term) void routeClipboardToTerm(term);
  }, [termRef]);

  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady) return;
    if (!connectedPtyRef.current) return;

    // Keyboard paths.
    //
    // Ctrl+V / Cmd+V — explicit handler that calls routeClipboardToTerm
    // and preventDefaults the keydown. The DOM `paste` event listener
    // below is unreliable on Windows WebView2 for keyboard-triggered
    // pastes (often fires with empty clipboardData, or doesn't fire at
    // all when focus is on the xterm helper textarea), so we drive the
    // paste from the keydown directly instead of waiting for the browser
    // to synthesize a paste event.
    //
    // Ctrl+C / Cmd+C — copy ONLY when there's a selection; otherwise
    // pass through to xterm so SIGINT works.
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown") return true;
      const mod = isMac ? e.metaKey : e.ctrlKey;
      if (!mod || e.altKey) return true;
      if (isMac && e.ctrlKey && e.metaKey) return true;

      if (e.code === "KeyV" && !e.shiftKey) {
        e.preventDefault();
        e.stopPropagation();
        void routeClipboardToTerm(term);
        return false;
      }

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

    // Safety net for paste events we didn't originate from the keydown
    // path — WebView2 native context menu "Paste", programmatic
    // execCommand("paste"), etc. Capture phase + stopImmediatePropagation
    // cancels xterm.js's own bubble-phase listener (#1803, #3516) so
    // routeClipboardToTerm fires exactly once.
    const el = containerRef.current;
    const onPaste = (ev: Event) => {
      ev.stopImmediatePropagation();
      ev.preventDefault();
      void routeClipboardToTerm(term);
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

  return { copySelection, pasteFromClipboard };
}
