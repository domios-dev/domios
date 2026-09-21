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
  hasResizeOwnership: () => boolean;
  refitAndCommitResize: () => void;
}

const CLAIM_FAILURE_DEADLINE_MS = 10_000;

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
  hasResizeOwnership,
  refitAndCommitResize,
}: UseTerminalActivationResizeOptions): () => void {
  const currentRef = useRef({ active, termReady });
  currentRef.current = { active, termReady };
  const activationFrameRef = useRef<number | null>(null);
  const ownedRefitFrameRef = useRef<number | null>(null);
  const requestRef = useRef(0);
  const pendingRequestRef = useRef<number | null>(null);
  const claimDeadlineRef = useRef<{ request: number; timer: ReturnType<typeof setTimeout> } | null>(null);
  const waitingForMeasureRef = useRef(false);

  const clearClaimDeadline = useCallback((request?: number) => {
    const pending = claimDeadlineRef.current;
    if (!pending || (request !== undefined && pending.request !== request)) return;
    clearTimeout(pending.timer);
    claimDeadlineRef.current = null;
  }, []);

  const cancelActivation = useCallback(() => {
    requestRef.current++;
    pendingRequestRef.current = null;
    clearClaimDeadline();
    waitingForMeasureRef.current = false;
    if (activationFrameRef.current !== null) cancelAnimationFrame(activationFrameRef.current);
    activationFrameRef.current = null;
    if (ownedRefitFrameRef.current !== null) cancelAnimationFrame(ownedRefitFrameRef.current);
    ownedRefitFrameRef.current = null;
    loseResizeOwnership();
  }, [clearClaimDeadline, loseResizeOwnership]);

  const activate = useCallback(() => {
    if (!currentRef.current.active || !currentRef.current.termReady || !documentVisible()) return;
    if (activationFrameRef.current !== null || pendingRequestRef.current !== null) return;
    activationFrameRef.current = requestAnimationFrame(() => {
      activationFrameRef.current = null;
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
      clearClaimDeadline();
      const timer = setTimeout(() => {
        if (claimDeadlineRef.current?.request !== request) return;
        claimDeadlineRef.current = null;
        if (pendingRequestRef.current !== request) return;
        pendingRequestRef.current = null;
        if (requestRef.current === request) loseResizeOwnership();
      }, CLAIM_FAILURE_DEADLINE_MS);
      claimDeadlineRef.current = { request, timer };
      void claimResizeOwner(tabId, currentPtyId, resizeClientId, term.cols, term.rows, true)
        .then((claim) => {
          const isPending = pendingRequestRef.current === request;
          clearClaimDeadline(request);
          if (isPending) pendingRequestRef.current = null;
          if (!isPending || requestRef.current !== request || !currentRef.current.active || !documentVisible()) return;
          setResizeOwner(claim.owner, currentPtyId, claim.current_size);
          if (claim.owner.client_id !== resizeClientId) return;
          refitAndCommitResize();
        })
        .catch(() => {
          const isPending = pendingRequestRef.current === request;
          clearClaimDeadline(request);
          if (isPending) pendingRequestRef.current = null;
          if (requestRef.current === request) loseResizeOwnership();
        });
    });
  }, [
    connectedPtyRef,
    clearClaimDeadline,
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

  const refitIfOwned = useCallback(() => {
    if (!currentRef.current.active || !currentRef.current.termReady || !documentVisible()) return;
    if (ownedRefitFrameRef.current !== null) return;
    ownedRefitFrameRef.current = requestAnimationFrame(() => {
      ownedRefitFrameRef.current = null;
      if (!currentRef.current.active || !currentRef.current.termReady || !documentVisible()) return;
      if (hasResizeOwnership()) refitAndCommitResize();
    });
  }, [hasResizeOwnership, refitAndCommitResize]);

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
    const onWindowFocus = () => activate();
    const onReconnect = () => refitIfOwned();
    document.addEventListener("visibilitychange", onVisibilityChange);
    window.addEventListener("focus", onWindowFocus);
    window.addEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, onReconnect);
    return () => {
      document.removeEventListener("visibilitychange", onVisibilityChange);
      window.removeEventListener("focus", onWindowFocus);
      window.removeEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, onReconnect);
    };
  }, [activate, cancelActivation, refitIfOwned]);

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
