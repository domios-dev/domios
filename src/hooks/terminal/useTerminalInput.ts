import { useEffect, useCallback, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import { isMac } from "../../lib/keybindings";
import { routeClipboardToTerm } from "../../lib/clipboardPaste";
import { createTerminalReplyFilter } from "./terminalReplyFilter";
import { writeTerminalInput } from "./terminalInputWrite";

export interface UseTerminalInputOptions {
  termRef: React.MutableRefObject<Terminal | null>;
  connectedPtyRef: React.MutableRefObject<string | null>;
  containerRef: React.RefObject<HTMLDivElement | null>;
  termReady: boolean;
  terminalActive: boolean;
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
  terminalActive,
  onTabKey,
  onNonTabKey,
  onInputData,
}: UseTerminalInputOptions): UseTerminalInputReturn {
  const terminalReplyFilterRef = useRef(createTerminalReplyFilter());
  const terminalReplyFilterPtyRef = useRef<string | null>(null);
  const terminalReplyFlushTimerRef = useRef<number | null>(null);

  const copySelection = useCallback(() => {
    const selection = termRef.current?.getSelection();
    if (selection) navigator.clipboard.writeText(selection).catch(() => {});
  }, [termRef]);

  // route as the keyboard path so file/image/text handling is consistent.
  const pasteFromClipboard = useCallback(() => {
    const term = termRef.current;
    if (term) void routeClipboardToTerm(term);
  }, [termRef]);

  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady) return;
    // No connectedPtyRef guard — PTY connects async; onData checks the ref at write time.

    // Ctrl+V/Cmd+V: handle paste at keydown so we don't depend on WebView2's flaky `paste` event.
    term.attachCustomKeyEventHandler((e) => {
      if (!terminalActive) return false;
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
      const currentPty = connectedPtyRef.current;
      if (terminalReplyFilterPtyRef.current !== currentPty) {
        if (terminalReplyFlushTimerRef.current !== null) {
          window.clearTimeout(terminalReplyFlushTimerRef.current);
          terminalReplyFlushTimerRef.current = null;
        }
        terminalReplyFilterRef.current.reset();
        terminalReplyFilterPtyRef.current = currentPty;
      }
      if (terminalReplyFlushTimerRef.current !== null) {
        window.clearTimeout(terminalReplyFlushTimerRef.current);
        terminalReplyFlushTimerRef.current = null;
      }
      const filtered = terminalReplyFilterRef.current.push(data);
      if (filtered) forwardInput(filtered, currentPty);
      const flushPty = currentPty;
      terminalReplyFlushTimerRef.current = window.setTimeout(() => {
        terminalReplyFlushTimerRef.current = null;
        if (terminalReplyFilterPtyRef.current !== flushPty || connectedPtyRef.current !== flushPty) {
          terminalReplyFilterRef.current.reset();
          return;
        }
        const flushed = terminalReplyFilterRef.current.flushStandaloneEsc();
        if (flushed) forwardInput(flushed, flushPty);
      }, 25);
    });

    const forwardInput = (data: string, currentPty: string | null) => {
      if (data === "\t" && onTabKey()) return;
      if (data !== "\t") onNonTabKey();
      onInputData(data);
      if (currentPty) void writeTerminalInput(currentPty, data);
    };

    // execCommand("paste"), etc. Capture phase + stopImmediatePropagation cancels xterm.js's own bubble-phase listener (#1803, #3516) so routeClipboardToTerm fires exactly once.
    const el = containerRef.current;
    const onPaste = (ev: Event) => {
      ev.stopImmediatePropagation();
      ev.preventDefault();
      void routeClipboardToTerm(term);
    };
    el?.addEventListener("paste", onPaste, true);

    const focusXterm = () => { if (terminalActive) term.focus(); };
    el?.addEventListener("mousedown", focusXterm);

    return () => {
      if (terminalReplyFlushTimerRef.current !== null) {
        window.clearTimeout(terminalReplyFlushTimerRef.current);
        terminalReplyFlushTimerRef.current = null;
      }
      onDataDisposable.dispose();
      el?.removeEventListener("mousedown", focusXterm);
      el?.removeEventListener("paste", onPaste, true);
    };
  }, [termReady, terminalActive, termRef, connectedPtyRef, containerRef, onTabKey, onNonTabKey, onInputData]);

  return { copySelection, pasteFromClipboard };
}
