import { useEffect, useRef, useCallback } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { invoke } from "../../lib/bridge";

export interface UseTerminalResizeOptions {
  containerRef: React.RefObject<HTMLDivElement | null>;
  termRef: React.MutableRefObject<Terminal | null>;
  fitAddonRef: React.MutableRefObject<FitAddon | null>;
  connectedPtyRef: React.MutableRefObject<string | null>;
  termReady: boolean;
  isFocused: boolean;
  resizeEnabled?: boolean;
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
    if (!termReady || !resizeEnabled || !containerRef.current) return;

    // Debounced refit (150ms — prevents thrashing during split drag).
    // Skips when the container is currently hidden (clientWidth ≈ 0) —
    // calling fit() against a 0-width box collapses xterm.cols to 1
    // and corrupts the visible buffer when the container becomes
    // visible again. The visibility:hidden trick in TerminalPane.tsx
    // keeps clientWidth non-zero on toggle, but this guard catches any
    // other path that ends up here while the element is collapsed.
    let resizeTimer: ReturnType<typeof setTimeout> | null = null;
    const safefit = () => {
      if (resizeTimer) clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        resizeTimer = null;
        const el = containerRef.current;
        if (!el || el.clientWidth < 20 || el.clientHeight < 20) return;
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
  }, [termReady, resizeEnabled, containerRef, termRef, fitAddonRef, connectedPtyRef, jiggleResize]);

  // Refit + focus when pane becomes focused
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
