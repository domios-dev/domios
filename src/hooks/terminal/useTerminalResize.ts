import { useCallback, useEffect, useRef } from "react";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { invoke, IS_WEB, listen } from "../../lib/bridge";
import { resizeOwnerChangedEvent, type ResizeOwner } from "../../lib/resizeOwner";
import { tabResizeChanged } from "./tabResize";

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

interface TerminalSize {
  ptyId: string;
  cols: number;
  rows: number;
}

const nativeResizeCommit = !IS_WEB
  && typeof navigator !== "undefined"
  && navigator.platform.startsWith("Win");

function sameSize(left: TerminalSize | null, right: TerminalSize): boolean {
  return left?.ptyId === right.ptyId && left.cols === right.cols && left.rows === right.rows;
}

export function resizeCommitRequired(
  committed: TerminalSize | null,
  next: TerminalSize,
  force: boolean,
): boolean {
  return force || !sameSize(committed, next);
}

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
  const committedSizeRef = useRef<TerminalSize | null>(null);
  const nativeResizeActiveRef = useRef(false);
  const ownsResizeRef = useRef(false);
  const suppressCommitRef = useRef(false);

  const commitResize = useCallback((force = false) => {
    if (!ownsResizeRef.current || suppressCommitRef.current) return;
    if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
    const term = termRef.current;
    const currentPtyId = connectedPtyRef.current ?? ptyId ?? null;
    if (!term || !currentPtyId || !resizeClientId) return;
    const size = pendingSizeRef.current ?? { cols: term.cols, rows: term.rows };
    if (size.cols <= 0 || size.rows <= 0) return;
    const next = { ptyId: currentPtyId, ...size };
    if (!resizeCommitRequired(committedSizeRef.current, next, force)) return;
    committedSizeRef.current = next;
    void invoke("resize_pty", {
      ptyId: currentPtyId,
      cols: size.cols,
      rows: size.rows,
      clientId: resizeClientId,
    }).catch(() => {
      if (sameSize(committedSizeRef.current, next)) committedSizeRef.current = null;
    });
  }, [connectedPtyRef, ptyId, resizeClientId, termRef]);

  const loseResizeOwnership = useCallback(() => {
    ownsResizeRef.current = false;
    committedSizeRef.current = null;
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
    committedSizeRef.current = isOwner && currentSize
      ? { ptyId: ownerPtyId, ...currentSize }
      : null;
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
    ownsResizeRef.current = false;
    committedSizeRef.current = null;
  }, [ptyId, resizeClientId]);

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
    let disposed = false;
    void listen("terminal-window-resize-start", () => {
      nativeResizeActiveRef.current = true;
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlistenStart = cleanup;
    });
    void listen("terminal-window-resize-commit", () => {
      nativeResizeActiveRef.current = false;
      if (commitFrame !== null) cancelAnimationFrame(commitFrame);
      commitFrame = requestAnimationFrame(() => {
        commitFrame = null;
        fitMeasuredContainer();
        commitResize();
      });
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlistenCommit = cleanup;
    });
    return () => {
      disposed = true;
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
