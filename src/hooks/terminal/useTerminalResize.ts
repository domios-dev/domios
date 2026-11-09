import { useCallback, useEffect, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { invoke, listen } from "../../lib/bridge";
import { nativeBridge } from "../../lib/nativeBridge";
import { resizeOwnerChangedEvent, type ResizeOwner } from "../../lib/resizeOwner";
import { tabResizeChanged } from "./tabResize";
import { completeResize, createResizeModel, ptyResizeGeometry, requestResize, setResizeAuthority, terminalGeometryPolicy, type ResizeRequest } from "./resizeModel";
import { authorityAfterClaim, authorityAfterOwnerChanged, unclaimedAuthority, type ViewerAuthority } from "./resizeAuthority";
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
  isRemote?: boolean;
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
  isRemote = false,
}: UseTerminalResizeOptions): UseTerminalResizeReturn {
  const lastContainerSizeRef = useRef({ w: 0, h: 0 });
  const pendingSizeRef = useRef<{ cols: number; rows: number } | null>(null);
  const resizeModelRef = useRef(createResizeModel());
  const authorityRef = useRef<ViewerAuthority>(unclaimedAuthority());
  const nativeResizeActiveRef = useRef(false);
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
    const policy = terminalGeometryPolicy(isRemote);
    if (policy === "viewer-local") return;
    if (authorityRef.current.role !== "owner" || suppressCommitRef.current) return;
    if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
    const term = termRef.current;
    const currentPtyId = connectedPtyRef.current ?? ptyId ?? null;
    if (!term || !currentPtyId || !resizeClientId) return;
    const size = ptyResizeGeometry(policy, pendingSizeRef.current ?? { cols: term.cols, rows: term.rows });
    if (!size) return;
    if (size.cols <= 0 || size.rows <= 0) return;
    const state = resizeModelRef.current;
    if (state.owner?.ptyId !== currentPtyId || state.owner.clientId !== resizeClientId) return;
    if (DEV_BUILD) console.debug("[resize-queue] requested", { ptyId: currentPtyId, ...size, generation: state.generation });
    resizeModelRef.current = requestResize(state, size, force);
    const request = resizeModelRef.current.inFlight;
    if (request && request !== state.inFlight) sendResize(request);
  }, [connectedPtyRef, isRemote, ptyId, resizeClientId, sendResize, termRef]);

  // Owner geometry is this view's fit; a follower renders the owner's size and
  // scrolls whatever does not fit its own box instead of refitting the PTY.
  const applyGeometry = useCallback((force: boolean) => {
    const el = containerRef.current;
    const term = termRef.current;
    if (!el || !term || el.clientWidth < 20 || el.clientHeight < 20) return;
    const w = el.clientWidth;
    const h = el.clientHeight;
    const authority = authorityRef.current;
    const followed = authority.role === "follower" ? authority.followed : null;
    if (!force && !followed && !tabResizeChanged(lastContainerSizeRef.current, w, h)) return;
    lastContainerSizeRef.current = { w, h };
    try {
      if (followed) {
        if (term.cols !== followed.cols || term.rows !== followed.rows) term.resize(followed.cols, followed.rows);
      } else {
        fitAddonRef.current?.fit();
      }
      if (force) term.refresh(0, term.rows - 1);
      pendingSizeRef.current = { cols: term.cols, rows: term.rows };
    } catch { /* teardown */ }
    el.dataset.geometry = followed ? "follow" : "fit";
  }, [containerRef, fitAddonRef, termRef]);

  const resetAuthority = useCallback(() => {
    authorityRef.current = unclaimedAuthority();
    pendingSizeRef.current = null;
    resizeModelRef.current = setResizeAuthority(resizeModelRef.current, null);
  }, []);

  // A follower keeps the owner's geometry it already knows; only an owner steps down.
  const loseResizeOwnership = useCallback(() => {
    if (authorityRef.current.role !== "owner") return;
    resetAuthority();
  }, [resetAuthority]);

  const hasResizeOwnership = useCallback(() => authorityRef.current.role === "owner", []);

  const setResizeOwner = useCallback((
    owner: ResizeOwner,
    ownerPtyId: string,
    currentSize: { cols: number; rows: number } | null,
  ) => {
    const self = { clientId: resizeClientId ?? "", ptyId: connectedPtyRef.current ?? ptyId ?? "" };
    const authority = self.ptyId === ownerPtyId
      ? authorityAfterClaim({
        ownerClientId: owner.client_id,
        ptyId: owner.pty_id,
        ownerSize: { cols: owner.cols, rows: owner.rows },
        currentSize,
      }, self)
      : unclaimedAuthority();
    authorityRef.current = authority;
    pendingSizeRef.current = null;
    resizeModelRef.current = setResizeAuthority(
      resizeModelRef.current,
      authority.role === "owner" && resizeClientId ? { ptyId: ownerPtyId, clientId: resizeClientId } : null,
      currentSize,
    );
    if (authority.role === "follower") applyGeometry(true);
  }, [applyGeometry, connectedPtyRef, ptyId, resizeClientId]);

  const refitAndCommitResize = useCallback(() => {
    suppressCommitRef.current = true;
    try {
      applyGeometry(true);
    } finally {
      suppressCommitRef.current = false;
    }
    if (!pendingSizeRef.current) return;
    commitResize();
  }, [applyGeometry, commitResize]);

  useEffect(() => {
    resetAuthority();
    return resetAuthority;
  }, [resetAuthority, ptyId, resizeClientId, resizeEnabled, tabId]);

  useEffect(() => {
    // Mesh viewers reflow locally and never follow or hold the shared PTY size.
    if (!resizeEnabled || terminalGeometryPolicy(isRemote) === "viewer-local") return;
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void listen<ResizeOwner>(resizeOwnerChangedEvent(tabId), (event) => {
      const owner = event.payload;
      if (!owner) return;
      const self = { clientId: resizeClientId ?? "", ptyId: connectedPtyRef.current ?? ptyId ?? "" };
      const next = authorityAfterOwnerChanged(authorityRef.current, owner, self);
      if (next === authorityRef.current) return;
      authorityRef.current = next;
      pendingSizeRef.current = null;
      resizeModelRef.current = setResizeAuthority(resizeModelRef.current, null);
      applyGeometry(true);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [applyGeometry, connectedPtyRef, isRemote, ptyId, resizeClientId, resizeEnabled, tabId]);

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
        applyGeometry(false);
        commitResize();
      });
    };
    const observer = new ResizeObserver(scheduleFit);
    observer.observe(containerRef.current);
    scheduleFit();

    const handleTabResize = () => {
      applyGeometry(true);
      commitResize();
    };
    window.addEventListener("codeterm-tab-resize", handleTabResize);
    return () => {
      observer.disconnect();
      if (animationFrame !== null) cancelAnimationFrame(animationFrame);
      window.removeEventListener("codeterm-tab-resize", handleTabResize);
    };
  }, [applyGeometry, commitResize, containerRef, resizeEnabled, termReady]);

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
        applyGeometry(false);
        commitResize();
      });
    }) ?? null;
    return () => {
      if (commitFrame !== null) cancelAnimationFrame(commitFrame);
      unlistenStart?.();
      unlistenCommit?.();
    };
  }, [applyGeometry, commitResize, resizeEnabled, termReady]);

  useEffect(() => {
    if (!isFocused || !termReady || !resizeEnabled) return;
    const animationFrame = requestAnimationFrame(() => termRef.current?.focus());
    return () => cancelAnimationFrame(animationFrame);
  }, [isFocused, resizeEnabled, termReady, termRef]);

  return { commitResize, setResizeOwner, loseResizeOwnership, hasResizeOwnership, refitAndCommitResize };
}
