import { useCallback, useEffect, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { invoke, listen } from "../../lib/bridge";
import { nativeBridge } from "../../lib/nativeBridge";
import { resizeOwnerChangedEvent, type ResizeOwner } from "../../lib/resizeOwner";
import { tabResizeChanged } from "./tabResize";
import { completeResize, createResizeModel, requestResize, setResizeAuthority, type ResizeRequest } from "./resizeModel";
import { DEV_BUILD } from "../../lib/devBuild";

export interface UseTerminalResizeOptions {
  containerRef: React.RefObject<HTMLDivElement | null>;
  termRef: React.MutableRefObject<Terminal | null>;
  fitAddonRef: React.MutableRefObject<FitAddon | null>;
  connectedPtyRef: React.MutableRefObject<string | null>;
  tabId?: string;
  ptyId?: string | null;
  termReady: boolean;
  isFocused: boolean;
  resizeEnabled?: boolean;
  resizeClientId?: string;
}

export interface UseTerminalResizeReturn {
  commitResize: (force?: boolean) => void;
  setResizeOwner: (
    owner: ResizeOwner,
    ptyId: string,
    currentSize: { cols: number; rows: number } | null,
  ) => void;
  loseResizeOwnership: () => void;
  hasResizeOwnership: () => boolean;
  refitAndCommitResize: () => void;
}

const nativeResizeCommit = Boolean(nativeBridge.resizeEvents)
  && typeof navigator !== "undefined"
  && navigator.platform.startsWith("Win");

export function useTerminalResize({
  containerRef,
  termRef,
  fitAddonRef,
  connectedPtyRef,
  tabId,
  ptyId,
  termReady,
  isFocused,
  resizeEnabled = true,
  resizeClientId,
}: UseTerminalResizeOptions): UseTerminalResizeReturn {
  const lastContainerSizeRef = useRef({ w: 0, h: 0 });
  const pendingSizeRef = useRef<{ cols: number; rows: number } | null>(null);
  const resizeModelRef = useRef(createResizeModel());
  const nativeResizeActiveRef = useRef(false);
  const ownsResizeRef = useRef(false);
  const suppressCommitRef = useRef(false);

  const sendResize = useCallback(function send(request: ResizeRequest) {
    if (DEV_BUILD) console.debug("[resize-queue] sending", request);
    void invoke("resize_pty", {
      ptyId: request.ptyId,
      cols: request.cols,
      rows: request.rows,
      clientId: request.clientId,
    }).then(() => finish(true), () => finish(false));

    function finish(succeeded: boolean) {
      if (DEV_BUILD) console.debug("[resize-queue] completed", { ...request, succeeded });
      resizeModelRef.current = completeResize(resizeModelRef.current, request, succeeded);
      const next = resizeModelRef.current.inFlight;
      if (next && next !== request) send(next);
    }
  }, []);

  const commitResize = useCallback((force = false) => {
    if (!ownsResizeRef.current || suppressCommitRef.current) return;
    if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
    const term = termRef.current;
    const currentPtyId = connectedPtyRef.current ?? ptyId ?? null;
    if (!term || !currentPtyId || !resizeClientId) return;
    const size = pendingSizeRef.current ?? { cols: term.cols, rows: term.rows };
    if (size.cols <= 0 || size.rows <= 0) return;
    const state = resizeModelRef.current;
    if (state.owner?.ptyId !== currentPtyId || state.owner.clientId !== resizeClientId) return;
    if (DEV_BUILD) console.debug("[resize-queue] requested", { ptyId: currentPtyId, ...size, generation: state.generation });
    resizeModelRef.current = requestResize(state, size, force);
    const request = resizeModelRef.current.inFlight;
    if (request && request !== state.inFlight) sendResize(request);
  }, [connectedPtyRef, ptyId, resizeClientId, sendResize, termRef]);

  const loseResizeOwnership = useCallback(() => {
    ownsResizeRef.current = false;
    pendingSizeRef.current = null;
    resizeModelRef.current = setResizeAuthority(resizeModelRef.current, null);
  }, []);

  const hasResizeOwnership = useCallback(() => ownsResizeRef.current, []);

  const setResizeOwner = useCallback((
    owner: ResizeOwner,
    ownerPtyId: string,
    currentSize: { cols: number; rows: number } | null,
  ) => {
    const isOwner = owner.client_id === resizeClientId
      && owner.pty_id === ownerPtyId
      && (connectedPtyRef.current ?? ptyId ?? null) === ownerPtyId;
    ownsResizeRef.current = isOwner;
    pendingSizeRef.current = null;
    resizeModelRef.current = setResizeAuthority(
      resizeModelRef.current,
      isOwner && resizeClientId ? { ptyId: ownerPtyId, clientId: resizeClientId } : null,
      currentSize,
    );
  }, [connectedPtyRef, ptyId, resizeClientId]);

  const fitMeasuredContainer = useCallback(() => {
    const el = containerRef.current;
    if (!el || el.clientWidth < 20 || el.clientHeight < 20) return;
    const w = el.clientWidth;
    const h = el.clientHeight;
    if (!tabResizeChanged(lastContainerSizeRef.current, w, h)) return;
    lastContainerSizeRef.current = { w, h };
    try { fitAddonRef.current?.fit(); } catch { /* teardown */ }
  }, [containerRef, fitAddonRef]);

  const forceFitMeasuredContainer = useCallback(() => {
    const el = containerRef.current;
    const term = termRef.current;
    if (!el || !term || el.clientWidth < 20 || el.clientHeight < 20) return;
    lastContainerSizeRef.current = { w: el.clientWidth, h: el.clientHeight };
    try {
      fitAddonRef.current?.fit();
      term.refresh(0, term.rows - 1);
    } catch { /* teardown */ }
  }, [containerRef, fitAddonRef, termRef]);

  const refitAndCommitResize = useCallback(() => {
    const el = containerRef.current;
    if (!el || el.clientWidth < 20 || el.clientHeight < 20) return;
    suppressCommitRef.current = true;
    try {
      forceFitMeasuredContainer();
    } finally {
      suppressCommitRef.current = false;
    }
    const term = termRef.current;
    if (!term) return;
    pendingSizeRef.current = { cols: term.cols, rows: term.rows };
    commitResize();
  }, [commitResize, containerRef, forceFitMeasuredContainer, termRef]);

  useEffect(() => {
    loseResizeOwnership();
    return loseResizeOwnership;
  }, [loseResizeOwnership, ptyId, resizeClientId, resizeEnabled, tabId]);

  useEffect(() => {
    if (!resizeEnabled) return;
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void listen<ResizeOwner>(resizeOwnerChangedEvent(tabId), (event) => {
      const owner = event.payload;
      const currentPtyId = connectedPtyRef.current ?? ptyId;
      if (owner?.pty_id !== currentPtyId || owner.client_id === resizeClientId) return;
      loseResizeOwnership();
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [connectedPtyRef, loseResizeOwnership, ptyId, resizeClientId, resizeEnabled, tabId]);

  useEffect(() => {
    const term = termRef.current;
    if (!term || !termReady || !resizeEnabled) return;
    const disposable = term.onResize(({ cols, rows }) => {
      pendingSizeRef.current = { cols, rows };
      if (!nativeResizeCommit) commitResize();
    });
    return () => disposable.dispose();
  }, [commitResize, resizeEnabled, termReady, termRef]);

  useEffect(() => {
    if (!termReady || !resizeEnabled || !containerRef.current) return;
    let animationFrame: number | null = null;
    const scheduleFit = () => {
      if (nativeResizeCommit && nativeResizeActiveRef.current) return;
      if (animationFrame !== null) return;
      animationFrame = requestAnimationFrame(() => {
        animationFrame = null;
        fitMeasuredContainer();
        commitResize();
      });
    };
    const observer = new ResizeObserver(scheduleFit);
    observer.observe(containerRef.current);
    scheduleFit();

    const handleTabResize = () => {
      forceFitMeasuredContainer();
      commitResize();
    };
    window.addEventListener("codeterm-tab-resize", handleTabResize);
    return () => {
      observer.disconnect();
      if (animationFrame !== null) cancelAnimationFrame(animationFrame);
      window.removeEventListener("codeterm-tab-resize", handleTabResize);
    };
  }, [commitResize, containerRef, fitMeasuredContainer, forceFitMeasuredContainer, resizeEnabled, termReady]);

  useEffect(() => {
    if (!nativeResizeCommit || !termReady || !resizeEnabled) return;
    let unlistenStart: (() => void) | null = null;
    let unlistenCommit: (() => void) | null = null;
    let commitFrame: number | null = null;
    unlistenStart = nativeBridge.resizeEvents?.onStart(() => {
      nativeResizeActiveRef.current = true;
    }) ?? null;
    unlistenCommit = nativeBridge.resizeEvents?.onCommit(() => {
      nativeResizeActiveRef.current = false;
      if (commitFrame !== null) cancelAnimationFrame(commitFrame);
      commitFrame = requestAnimationFrame(() => {
        commitFrame = null;
        fitMeasuredContainer();
        commitResize();
      });
    }) ?? null;
    return () => {
      if (commitFrame !== null) cancelAnimationFrame(commitFrame);
      unlistenStart?.();
      unlistenCommit?.();
    };
  }, [commitResize, fitMeasuredContainer, resizeEnabled, termReady]);

  useEffect(() => {
    if (!isFocused || !termReady || !resizeEnabled) return;
    const animationFrame = requestAnimationFrame(() => termRef.current?.focus());
    return () => cancelAnimationFrame(animationFrame);
  }, [isFocused, resizeEnabled, termReady, termRef]);

  return { commitResize, setResizeOwner, loseResizeOwnership, hasResizeOwnership, refitAndCommitResize };
}
