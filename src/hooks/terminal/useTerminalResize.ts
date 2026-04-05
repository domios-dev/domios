import { useEffect, useRef, useCallback } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { invoke } from "@tauri-apps/api/core";

export interface UseTerminalResizeOptions {
  containerRef: React.RefObject<HTMLDivElement | null>;
  termRef: React.MutableRefObject<Terminal | null>;
  fitAddonRef: React.MutableRefObject<FitAddon | null>;
  connectedPtyRef: React.MutableRefObject<string | null>;
  termReady: boolean;
  isFocused: boolean;
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
}: UseTerminalResizeOptions): UseTerminalResizeReturn {
  // ConPTY resize jiggle: shrink by 1 col then restore to force SIGWINCH
  const jigglePendingRef = useRef(false);
  const jiggleResize = useCallback((ptyId: string) => {
    const term = termRef.current;
    if (!term || jigglePendingRef.current) return;
    const cols = term.cols, rows = term.rows;
    if (cols <= 1 || rows <= 0) return;
    jigglePendingRef.current = true;
    invoke("resize_pty", { ptyId, cols: cols - 1, rows })
      .then(() => new Promise<void>(r => setTimeout(r, 100)))
      .then(() => invoke("resize_pty", { ptyId, cols, rows }))
      .finally(() => { jigglePendingRef.current = false; });
  }, [termRef]);

  // ResizeObserver + debounce + self-heal + HMR + visibility
  useEffect(() => {
    if (!termReady || !containerRef.current) return;

    // Debounced refit (150ms — prevents thrashing during split drag)
    let resizeTimer: ReturnType<typeof setTimeout> | null = null;
    const safefit = () => {
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        resizeTimer = null;
        if (fitAddonRef.current) {
          try { fitAddonRef.current.fit(); } catch { /* teardown */ }
        }
      }, 150);
    };

    const resizeObserver = new ResizeObserver(safefit);
    resizeObserver.observe(containerRef.current);

    // Self-heal: refit on window focus and visibility change
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

    // HMR: force refit + repaint after hot module replacement
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
          fitAddonRef.current?.fit();
          const term = termRef.current;
          const currentPtyId = connectedPtyRef.current;
          if (term && currentPtyId && term.cols > 0 && term.rows > 0) {
            invoke("resize_pty", { ptyId: currentPtyId, cols: term.cols, rows: term.rows });
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
  }, [termReady, containerRef, termRef, fitAddonRef, connectedPtyRef, jiggleResize]);

  // Refit + focus when pane becomes focused
  useEffect(() => {
    if (isFocused && termReady) {
      requestAnimationFrame(() => {
        try {
          fitAddonRef.current?.fit();
          termRef.current?.focus();
        } catch { /* ignore */ }
      });
    }
  }, [isFocused, termReady, fitAddonRef, termRef]);

  return { jiggleResize };
}
