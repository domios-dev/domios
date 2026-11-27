import { useEffect, useCallback, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import { isMac } from "../../lib/keybindings";
import { routeClipboardToTerm } from "../../lib/clipboardPaste";
import { writeViewerClipboardText } from "../../lib/clipboardWrite";
import { createTerminalReplyFilter } from "./terminalReplyFilter";
import { writeTerminalInput } from "./terminalInputWrite";
import { normalizeCopiedTerminalSelection } from "./terminalLinks";
import { evaluateOsc52ClipboardWrite } from "./terminalOsc52";

export interface UseTerminalInputOptions {
  tabId: string;
  termRef: React.MutableRefObject<Terminal | null>;
  connectedPtyRef: React.MutableRefObject<string | null>;
  containerRef: React.RefObject<HTMLDivElement | null>;
  termReady: boolean;
  terminalActive: boolean;
  readOnly?: boolean;
  isRemote?: boolean;
  replayingHistory?: boolean;
  onTabKey: () => boolean;
  onNonTabKey: () => void;
  onInputData: (data: string) => void;
}

export interface UseTerminalInputReturn {
  copySelection: () => void;
  pasteFromClipboard: () => void;
}

export function useTerminalInput({
  tabId,
  termRef,
  connectedPtyRef,
  containerRef,
  termReady,
  terminalActive,
  readOnly = false,
  isRemote = false,
  replayingHistory = false,
  onTabKey,
  onNonTabKey,
  onInputData,
}: UseTerminalInputOptions): UseTerminalInputReturn {
  const terminalReplyFilterRef = useRef(createTerminalReplyFilter());
  const terminalReplyFilterPtyRef = useRef<string | null>(null);
  const terminalReplyFlushTimerRef = useRef<number | null>(null);
  const lastUserIntentAtRef = useRef<number | null>(null);
  const osc52StateRef = useRef({ isRemote, terminalActive, replayingHistory });
  osc52StateRef.current = { isRemote, terminalActive, replayingHistory };

  const copySelection = useCallback(() => {
    const selection = termRef.current?.getSelection();
    if (!selection) return;
    void writeViewerClipboardText(normalizeCopiedTerminalSelection(selection));
  }, [termRef]);

  // route as the keyboard path so file/image/text handling is consistent.
  const pasteFromClipboard = useCallback(() => {
    const term = termRef.current;
    if (term && !readOnly) void routeClipboardToTerm(term);
  }, [termRef, readOnly]);

  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady || !term.parser?.registerOscHandler) return;
    const osc52Disposable = term.parser.registerOscHandler(52, (data) => {
      const state = osc52StateRef.current;
      const decision = evaluateOsc52ClipboardWrite(data, {
        isRemoteTab: state.isRemote,
        terminalActive: state.terminalActive,
        replayingHistory: state.replayingHistory,
        nowMs: Date.now(),
        lastUserIntentAtMs: lastUserIntentAtRef.current,
      });
      if (decision.action === "write") {
        void writeViewerClipboardText(decision.text);
      }
      return true;
    });
    return () => osc52Disposable.dispose();
  }, [termReady, termRef]);

  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady) return;
    // No connectedPtyRef guard — PTY connects async; onData checks the ref at write time.

    // Ctrl+V/Cmd+V: handle paste at keydown so we don't depend on WebView2's flaky `paste` event.
    term.attachCustomKeyEventHandler((e) => {
      if (!terminalActive) return false;
      if (e.type !== "keydown") return !readOnly;
      lastUserIntentAtRef.current = Date.now();
      const mod = isMac ? e.metaKey : e.ctrlKey;
      if (!mod || e.altKey) return !readOnly;
      if (isMac && e.ctrlKey && e.metaKey) return !readOnly;

      if (e.code === "KeyC") {
        const sel = term.getSelection();
        if (sel) {
          e.preventDefault();
          e.stopPropagation();
          void writeViewerClipboardText(normalizeCopiedTerminalSelection(sel));
          if (!e.shiftKey) term.clearSelection();
          return false;
        }
        if (e.shiftKey) return false;
      }

      if (readOnly) return false;

      if (e.code === "KeyV") {
        e.preventDefault();
        e.stopPropagation();
        void routeClipboardToTerm(term);
        return false;
      }

      return true;
    });

    const onDataDisposable = term.onData((data) => {
      if (!terminalActive || readOnly) return;
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
      lastUserIntentAtRef.current = Date.now();
      if (data === "\t" && onTabKey()) return;
      if (data !== "\t") onNonTabKey();
      onInputData(data);
      if (currentPty) void writeTerminalInput(tabId, currentPty, data);
    };

    // execCommand("paste"), etc. Capture phase + stopImmediatePropagation cancels xterm.js's own bubble-phase listener (#1803, #3516) so routeClipboardToTerm fires exactly once.
    const el = containerRef.current;
    const onPaste = (ev: Event) => {
      ev.stopImmediatePropagation();
      ev.preventDefault();
      if (readOnly) return;
      const clipboardData = (ev as ClipboardEvent).clipboardData;
      const fallbackPlain = clipboardData?.getData("text/plain") || undefined;
      void routeClipboardToTerm(term, fallbackPlain);
    };
    const onCopy = (ev: Event) => {
      if (!terminalActive || !term.hasSelection?.()) return;
      const sel = term.getSelection();
      if (!sel) return;
      const clipboardData = (ev as ClipboardEvent).clipboardData;
      if (!clipboardData) return;
      ev.preventDefault();
      ev.stopImmediatePropagation();
      clipboardData.setData("text/plain", normalizeCopiedTerminalSelection(sel));
    };
    const markUserIntent = () => {
      if (terminalActive) lastUserIntentAtRef.current = Date.now();
    };
    el?.addEventListener("paste", onPaste, true);
    el?.addEventListener("copy", onCopy, true);
    el?.addEventListener("pointerdown", markUserIntent, true);

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
      el?.removeEventListener("copy", onCopy, true);
      el?.removeEventListener("pointerdown", markUserIntent, true);
    };
  }, [tabId, termReady, terminalActive, readOnly, termRef, connectedPtyRef, containerRef, onTabKey, onNonTabKey, onInputData]);

  return { copySelection, pasteFromClipboard };
}
