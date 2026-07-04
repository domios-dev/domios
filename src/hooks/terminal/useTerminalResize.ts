import { useEffect, useRef, useCallback } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { invoke } from "../../lib/bridge";
import { paneResizeChanged } from "./paneResize";

export interface UseTerminalResizeOptions {
  containerRef: React.RefObject<HTMLDivElement | null>;
  termRef: React.MutableRefObject<Terminal | null>;
  fitAddonRef: React.MutableRefObject<FitAddon | null>;
  connectedPtyRef: React.MutableRefObject<string | null>;
  termReady: boolean;
  isFocused: boolean;
  resizeEnabled?: boolean;
  resizeClientId?: string;
}

export interface UseTerminalResizeReturn {
  jiggleResize: (ptyId: string) => void;
}


export function useTerminalResize({
  containerRef,
  termRef,
  fitAddonRef,
  connectedPtyRef,
  termReady,
  isFocused,
  resizeEnabled = true,
  resizeClientId,
}: UseTerminalResizeOptions): UseTerminalResizeReturn {
  const jigglePendingRef = useRef(false);
  const lastPaneResizeDimsRef = useRef({ w: 0, h: 0 });
  const jiggleResize = useCallback((ptyId: string) => {
    const term = termRef.current;
    if (!term || jigglePendingRef.current) return;
    const cols = term.cols, rows = term.rows;
    if (cols <= 1 || rows <= 0) return;
    jigglePendingRef.current = true;
    invoke("resize_pty", { ptyId, cols: cols - 1, rows, clientId: resizeClientId })
      .then(() => new Promise<void>(r => setTimeout(r, 100)))
      .then(() => invoke("resize_pty", { ptyId, cols, rows, clientId: resizeClientId }))
      .finally(() => { jigglePendingRef.current = false; });
  }, [termRef, resizeClientId]);

  useEffect(() => {
    if (!termReady || !resizeEnabled || !containerRef.current) return;

    // Debounced refit after split drag only; skip while pane is hidden.
    // fit() on 0-width element collapses cols to 1 and distorts the buffer.
    let resizeTimer: ReturnType<typeof setTimeout> | null = null;
    const safefit = () => {
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        resizeTimer = null;
        const el = containerRef.current;
        if (!el || el.clientWidth < 20 || el.clientHeight < 20) return;
        lastPaneResizeDimsRef.current = { w: el.clientWidth, h: el.clientHeight };
        if (fitAddonRef.current) {
          try { fitAddonRef.current.fit(); } catch { /* teardown */ }
        }
      }, 150);
    };

    const resizeObserver = new ResizeObserver(safefit);
    resizeObserver.observe(containerRef.current);

    const handleVisibility = () => { if (!document.hidden) safefit(); };
    const handleFocus = () => safefit();
    document.addEventListener("visibilitychange", handleVisibility);
    window.addEventListener("focus", handleFocus);

    // Periodic self-heal: detect dimension drift (cheap check every 2s)
    const healTimer = setInterval(() => {
      const el = containerRef.current;
      if (!el || !fitAddonRef.current || !termRef.current) return;
      const term = termRef.current;
      const cellW = term.options.fontSize ?? 13;
      const expectedCols = Math.floor(el.clientWidth / (cellW * 0.6));
      if (expectedCols > 0 && Math.abs(term.cols - expectedCols) > 5) {
        safefit();
      }
    }, 2000);

    let hmrCleanup: (() => void) | undefined;
    if (import.meta.hot) {
      const onHmr = () => {
        requestAnimationFrame(() => {
          if (!fitAddonRef.current || !termRef.current) return;
          const t = termRef.current;
          const ptyId = connectedPtyRef.current;
          try {
            fitAddonRef.current.fit();
            t.refresh(0, t.rows - 1);
            if (ptyId) jiggleResize(ptyId);
          } catch { /* ignore */ }
        });
      };
      import.meta.hot.on("vite:afterUpdate", onHmr);
      hmrCleanup = () => import.meta.hot?.off("vite:afterUpdate", onHmr);
    }

    // Listen for pane resize events (triggered after divider drag ends)
    const handlePaneResize = () => {
      requestAnimationFrame(() => {
        try {
          const el = containerRef.current;
          if (!el) return;
          const w = el.clientWidth, h = el.clientHeight;
          if (!paneResizeChanged(lastPaneResizeDimsRef.current, w, h)) return;
          lastPaneResizeDimsRef.current = { w, h };
          fitAddonRef.current?.fit();
          const term = termRef.current;
          const currentPtyId = connectedPtyRef.current;
          if (term && currentPtyId && term.cols > 0 && term.rows > 0) {
            invoke("resize_pty", {
              ptyId: currentPtyId,
              cols: term.cols,
              rows: term.rows,
              clientId: resizeClientId,
            });
          }
        } catch { /* ignore */ }
      });
    };
    window.addEventListener("codeterm-pane-resize", handlePaneResize);

    return () => {
      resizeObserver.disconnect();
      if (resizeTimer) clearTimeout(resizeTimer);
      document.removeEventListener("visibilitychange", handleVisibility);
      window.removeEventListener("focus", handleFocus);
      clearInterval(healTimer);
      hmrCleanup?.();
      window.removeEventListener("codeterm-pane-resize", handlePaneResize);
    };
  }, [termReady, resizeEnabled, resizeClientId, containerRef, termRef, fitAddonRef, connectedPtyRef, jiggleResize]);

  useEffect(() => {
    if (isFocused && termReady && resizeEnabled) {
      requestAnimationFrame(() => {
        try {
          fitAddonRef.current?.fit();
          termRef.current?.focus();
        } catch { /* ignore */ }
      });
    }
  }, [isFocused, termReady, resizeEnabled, fitAddonRef, termRef]);

  return { jiggleResize };
}
