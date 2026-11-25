import { useEffect } from "react";
import { nativeBridge } from "../lib/nativeBridge";

const RESIZE_SETTLE_MS = 160;

/**
 * Publishes document-level performance state so CSS can pause infinite
 * animations while the window is hidden and suspend expensive backdrop-filter
 * readbacks during active window resize drags.
 */
export function useDocumentPerfState(): void {
  useEffect(() => {
    if (typeof document === "undefined" || typeof window === "undefined") return;
    const root = document.documentElement;
    const syncVisibility = () => {
      if (document.visibilityState === "hidden") root.dataset.motion = "paused";
      else if (root.dataset.motion === "paused") delete root.dataset.motion;
    };
    syncVisibility();
    document.addEventListener("visibilitychange", syncVisibility);

    let settleTimer: number | null = null;
    let nativeDragging = false;
    const clearSettle = () => {
      if (settleTimer !== null) {
        window.clearTimeout(settleTimer);
        settleTimer = null;
      }
    };
    const setResizing = (active: boolean) => {
      if (active) root.dataset.windowResizing = "true";
      else delete root.dataset.windowResizing;
    };
    const unlistenStart = nativeBridge.resizeEvents?.onStart(() => {
      nativeDragging = true;
      clearSettle();
      setResizing(true);
    });
    const unlistenCommit = nativeBridge.resizeEvents?.onCommit(() => {
      nativeDragging = false;
      clearSettle();
      setResizing(false);
    });
    const onWindowResize = () => {
      if (nativeDragging) return;
      setResizing(true);
      clearSettle();
      settleTimer = window.setTimeout(() => {
        settleTimer = null;
        if (!nativeDragging) setResizing(false);
      }, RESIZE_SETTLE_MS);
    };
    window.addEventListener("resize", onWindowResize, { passive: true });
    return () => {
      document.removeEventListener("visibilitychange", syncVisibility);
      window.removeEventListener("resize", onWindowResize);
      unlistenStart?.();
      unlistenCommit?.();
      clearSettle();
      delete root.dataset.windowResizing;
      if (root.dataset.motion === "paused") delete root.dataset.motion;
    };
  }, []);
}
