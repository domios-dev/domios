import { useCallback, useEffect, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { claimResizeOwner } from "../../lib/resizeOwner";
import type { ResizeOwner, ResizeOwnerClaim } from "../../lib/resizeOwner";
import { TERMINAL_VIEWER_RECONNECTED_EVENT } from "../../lib/terminalViewerEvents";
import { claimModeFor, type AuthorityTrigger, type ClaimMode } from "./resizeAuthority";

export interface UseTerminalActivationResizeOptions {
  active: boolean;
  viewerLocalGeometry?: boolean;
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

export type ActivateTerminal = (trigger?: AuthorityTrigger) => void;

const CLAIM_FAILURE_DEADLINE_MS = 10_000;

function documentVisible(): boolean {
  return typeof document === "undefined" || document.visibilityState !== "hidden";
}

function strongerClaim(current: ClaimMode | null, next: ClaimMode): ClaimMode {
  return current === "forced" ? current : next;
}

export function useTerminalActivationResize({
  active,
  viewerLocalGeometry = false,
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
}: UseTerminalActivationResizeOptions): ActivateTerminal {
  const currentRef = useRef({ active, termReady });
  currentRef.current = { active, termReady };
  const activationFrameRef = useRef<{ frame: number; mode: ClaimMode } | null>(null);
  const ownedRefitFrameRef = useRef<number | null>(null);
  const requestRef = useRef(0);
  const pendingRequestRef = useRef<{ request: number; mode: ClaimMode } | null>(null);
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
    if (activationFrameRef.current !== null) cancelAnimationFrame(activationFrameRef.current.frame);
    activationFrameRef.current = null;
    if (ownedRefitFrameRef.current !== null) cancelAnimationFrame(ownedRefitFrameRef.current);
    ownedRefitFrameRef.current = null;
    loseResizeOwnership();
  }, [clearClaimDeadline, loseResizeOwnership]);

  const proposeGeometry = useCallback((): { cols: number; rows: number } | null => {
    const term = termRef.current;
    if (!term) return null;
    const proposed = fitAddonRef.current?.proposeDimensions?.();
    const geometry = proposed ?? { cols: term.cols, rows: term.rows };
    return geometry.cols > 0 && geometry.rows > 0 ? geometry : null;
  }, [fitAddonRef, termRef]);

  const activate = useCallback<ActivateTerminal>((trigger = "activation") => {
    const mode = claimModeFor(trigger);
    if (mode === "none") return;
    if (!currentRef.current.active || !currentRef.current.termReady || !documentVisible()) return;
    const scheduled = activationFrameRef.current;
    if (scheduled) {
      scheduled.mode = strongerClaim(scheduled.mode, mode);
      return;
    }
    const pending = pendingRequestRef.current;
    if (pending) {
      // An explicit interaction outranks an adoption that is still in flight.
      if (mode !== "forced" || pending.mode === "forced") return;
      requestRef.current++;
      pendingRequestRef.current = null;
      clearClaimDeadline();
    }
    const frame = requestAnimationFrame(() => {
      const claimMode = activationFrameRef.current?.mode ?? mode;
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
      if (viewerLocalGeometry) {
        refitAndCommitResize();
        if (isFocused) term.focus();
        return;
      }
      if (claimMode === "forced" && hasResizeOwnership()) {
        refitAndCommitResize();
        return;
      }
      const geometry = proposeGeometry();
      if (!geometry) return;

      const request = ++requestRef.current;
      pendingRequestRef.current = { request, mode: claimMode };
      clearClaimDeadline();
      const timer = setTimeout(() => {
        if (claimDeadlineRef.current?.request !== request) return;
        claimDeadlineRef.current = null;
        if (pendingRequestRef.current?.request !== request) return;
        pendingRequestRef.current = null;
        if (requestRef.current === request) loseResizeOwnership();
      }, CLAIM_FAILURE_DEADLINE_MS);
      claimDeadlineRef.current = { request, timer };
      void claimResizeOwner(tabId, currentPtyId, resizeClientId, geometry.cols, geometry.rows, claimMode === "forced")
        .then((claim) => {
          const isPending = pendingRequestRef.current?.request === request;
          clearClaimDeadline(request);
          if (isPending) pendingRequestRef.current = null;
          if (!isPending || requestRef.current !== request || !currentRef.current.active || !documentVisible()) return;
          setResizeOwner(claim.owner, currentPtyId, claim.current_size);
          if (claim.owner.client_id !== resizeClientId) return;
          refitAndCommitResize();
        })
        .catch(() => {
          const isPending = pendingRequestRef.current?.request === request;
          clearClaimDeadline(request);
          if (isPending) pendingRequestRef.current = null;
          if (requestRef.current === request) loseResizeOwnership();
        });
    });
    activationFrameRef.current = { frame, mode };
  }, [
    connectedPtyRef,
    clearClaimDeadline,
    containerRef,
    hasResizeOwnership,
    loseResizeOwnership,
    proposeGeometry,
    ptyId,
    isFocused,
    refitAndCommitResize,
    resizeClientId,
    setResizeOwner,
    tabId,
    termRef,
    viewerLocalGeometry,
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
    activate("activation");
    return cancelActivation;
  }, [active, cancelActivation, ptyId, termReady, activate]);

  // Focus moving to this view is an activation, not an interaction: it adopts an
  // unowned PTY and otherwise follows, without dropping what this view already holds.
  useEffect(() => {
    if (isFocused) activate("activation");
  }, [activate, isFocused]);

  useEffect(() => {
    const onVisibilityChange = () => {
      if (!documentVisible()) cancelActivation();
      else activate("visibility");
    };
    const onWindowFocus = () => activate("window-focus");
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

  // Typing or pointing into this view is the explicit act that takes the PTY's size.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const onInteraction = () => activate("interaction");
    container.addEventListener("pointerdown", onInteraction);
    container.addEventListener("keydown", onInteraction);
    return () => {
      container.removeEventListener("pointerdown", onInteraction);
      container.removeEventListener("keydown", onInteraction);
    };
  }, [activate, containerRef]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (waitingForMeasureRef.current) activate("activation");
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, [activate, containerRef]);

  return activate;
}
