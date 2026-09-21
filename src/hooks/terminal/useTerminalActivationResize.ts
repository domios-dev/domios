import { useCallback, useEffect, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { claimResizeOwner } from "../../lib/resizeOwner";
import type { ResizeOwner, ResizeOwnerClaim } from "../../lib/resizeOwner";
import { TERMINAL_VIEWER_RECONNECTED_EVENT } from "../../lib/terminalViewerEvents";

export interface UseTerminalActivationResizeOptions {
  active: boolean;
  isFocused: boolean;
  termReady: boolean;
  tabId: string;
  ptyId?: string | null;
  resizeClientId: string;
  containerRef: React.RefObject<HTMLDivElement | null>;
  termRef: React.MutableRefObject<Terminal | null>;
  fitAddonRef: React.MutableRefObject<FitAddon | null>;
  connectedPtyRef: React.MutableRefObject<string | null>;
  setResizeOwner: (
    owner: ResizeOwner,
    ptyId: string,
    currentSize: ResizeOwnerClaim["current_size"],
  ) => void;
  loseResizeOwnership: () => void;
  refitAndCommitResize: () => void;
}

function documentVisible(): boolean {
  return typeof document === "undefined" || document.visibilityState !== "hidden";
}

export function useTerminalActivationResize({
  active,
  isFocused,
  termReady,
  tabId,
  ptyId,
  resizeClientId,
  containerRef,
  termRef,
  fitAddonRef,
  connectedPtyRef,
  setResizeOwner,
  loseResizeOwnership,
  refitAndCommitResize,
}: UseTerminalActivationResizeOptions): () => void {
  const currentRef = useRef({ active, termReady });
  currentRef.current = { active, termReady };
  const frameRef = useRef<number | null>(null);
  const requestRef = useRef(0);
  const pendingRequestRef = useRef<number | null>(null);
  const waitingForMeasureRef = useRef(false);

  const cancelActivation = useCallback(() => {
    requestRef.current++;
    pendingRequestRef.current = null;
    waitingForMeasureRef.current = false;
    if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    frameRef.current = null;
    loseResizeOwnership();
  }, [loseResizeOwnership]);

  const activate = useCallback(() => {
    if (!currentRef.current.active || !currentRef.current.termReady || !documentVisible()) return;
    if (frameRef.current !== null || pendingRequestRef.current !== null) return;
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      if (!currentRef.current.active || !currentRef.current.termReady || !documentVisible()) return;
      const container = containerRef.current;
      const term = termRef.current;
      const currentPtyId = connectedPtyRef.current ?? ptyId ?? null;
      if (!container || !term || !currentPtyId) return;
      if (container.clientWidth < 20 || container.clientHeight < 20) {
        waitingForMeasureRef.current = true;
        return;
      }
      waitingForMeasureRef.current = false;
      try { fitAddonRef.current?.fit(); } catch { return; }
      if (term.cols <= 0 || term.rows <= 0) return;

      const request = ++requestRef.current;
      pendingRequestRef.current = request;
      void claimResizeOwner(tabId, currentPtyId, resizeClientId, term.cols, term.rows, true)
        .then((claim) => {
          if (pendingRequestRef.current === request) pendingRequestRef.current = null;
          if (requestRef.current !== request || !currentRef.current.active || !documentVisible()) return;
          setResizeOwner(claim.owner, currentPtyId, claim.current_size);
          if (claim.owner.client_id !== resizeClientId) return;
          refitAndCommitResize();
        })
        .catch(() => {
          if (pendingRequestRef.current === request) pendingRequestRef.current = null;
          if (requestRef.current === request) loseResizeOwnership();
        });
    });
  }, [
    connectedPtyRef,
    containerRef,
    fitAddonRef,
    loseResizeOwnership,
    ptyId,
    refitAndCommitResize,
    resizeClientId,
    setResizeOwner,
    tabId,
    termRef,
  ]);

  useEffect(() => {
    if (!active) {
      cancelActivation();
      return;
    }
    activate();
    return cancelActivation;
  }, [active, cancelActivation, isFocused, ptyId, termReady, activate]);

  useEffect(() => {
    const onVisibilityChange = () => {
      if (!documentVisible()) cancelActivation();
      else activate();
    };
    const onWindowBlur = () => cancelActivation();
    const onWindowFocus = () => activate();
    const onReconnect = () => activate();
    document.addEventListener("visibilitychange", onVisibilityChange);
    window.addEventListener("blur", onWindowBlur);
    window.addEventListener("focus", onWindowFocus);
    window.addEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, onReconnect);
    return () => {
      document.removeEventListener("visibilitychange", onVisibilityChange);
      window.removeEventListener("blur", onWindowBlur);
      window.removeEventListener("focus", onWindowFocus);
      window.removeEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, onReconnect);
    };
  }, [activate, cancelActivation]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (waitingForMeasureRef.current) activate();
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, [activate, containerRef]);

  return activate;
}
