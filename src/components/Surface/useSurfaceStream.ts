import { useCallback, useEffect, useRef, useState } from "react";
import { createFrameFeed, type FrameFeed } from "./frameFeed";
import { frameMatchesViewport, viewportAction, type FrameMetadata, type StreamViewport } from "./streamModel";
import type { SurfaceState } from "./surfaceChromeModel";
import { isPreviewPaused, resizeArgs, SURFACE_METHOD, type Disconnect, type StreamFrame, type SurfaceNotice, type SurfaceStreamTransport } from "./surfaceStreamTransport";

export interface SurfaceStream {
  state: SurfaceState;
  notice: SurfaceNotice | null;
  /** Set by a preview_paused notice and cleared by the next frame. */
  paused: SurfaceNotice | null;
  frames: FrameFeed;
  setNotice(notice: SurfaceNotice | null): void;
  onViewport(viewport: StreamViewport): void;
  onHidden(): void;
}

interface Connection {
  disconnect: Disconnect | null;
  sent: StreamViewport | null;
  latest: StreamViewport | null;
  healedAt: number;
}

export const FRAME_HEAL_INTERVAL_MS = 3000;

// Only the visible, focused window may claim the viewport every viewer shares.
function documentActive(): boolean {
  if (typeof document === "undefined") return false;
  const focused = typeof document.hasFocus === "function" ? document.hasFocus() : true;
  return document.visibilityState === "visible" && focused;
}

export function useSurfaceStream(transport: SurfaceStreamTransport, initialState: SurfaceState): SurfaceStream {
  const [state, setState] = useState<SurfaceState>(initialState);
  const [notice, setNotice] = useState<SurfaceNotice | null>(null);
  const [paused, setPaused] = useState<SurfaceNotice | null>(null);
  const pausedRef = useRef(false);
  const [feed] = useState(createFrameFeed);
  const connection = useRef<Connection>({ disconnect: null, sent: null, latest: null, healedAt: Number.NEGATIVE_INFINITY });

  const sendResize = useCallback((viewport: StreamViewport) => {
    connection.current.sent = viewport;
    transport.invoke(SURFACE_METHOD.resize, resizeArgs(viewport)).catch((error: unknown) => {
      setNotice({ text: error instanceof Error ? error.message : String(error), tone: "error" });
    });
  }, [transport]);

  const heal = useCallback((meta: FrameMetadata) => {
    const current = connection.current;
    // sent is null while the stage is hidden; a hidden stage must not claim the shared viewport.
    if (!current.latest || !current.sent || frameMatchesViewport(meta, current.latest) || !documentActive()) return;
    const now = Date.now();
    if (now - current.healedAt <= FRAME_HEAL_INTERVAL_MS) return;
    current.healedAt = now;
    sendResize(current.latest);
  }, [sendResize]);

  const connect = useCallback((viewport: StreamViewport) => {
    const current = connection.current;
    current.sent = viewport;
    current.disconnect = transport.connect(viewport, {
      frame: (frame: StreamFrame) => {
        feed.publish(frame);
        heal(frame.metadata);
        if (pausedRef.current) {
          pausedRef.current = false;
          setPaused(null);
        }
      },
      state: setState,
      notice: (next: SurfaceNotice) => {
        if (!isPreviewPaused(next)) {
          setNotice(next);
          return;
        }
        pausedRef.current = true;
        setPaused(next);
      },
      // A reconnect reuses the original connect params, so the current size is re-sent on every open.
      open: () => { if (current.latest) sendResize(current.latest); },
    });
  }, [transport, feed, sendResize, heal]);

  useEffect(() => {
    const current = connection.current;
    if (current.latest) connect(current.latest);
    return () => {
      current.disconnect?.();
      current.disconnect = null;
      current.sent = null;
    };
  }, [connect]);

  const onViewport = useCallback((viewport: StreamViewport) => {
    const current = connection.current;
    current.latest = viewport;
    const action = viewportAction(current.disconnect !== null, current.sent, viewport);
    if (action === "connect") connect(viewport);
    else if (action === "resize") sendResize(viewport);
  }, [connect, sendResize]);

  const onHidden = useCallback(() => {
    connection.current.sent = null;
  }, []);

  return { state, notice, paused, frames: feed, setNotice, onViewport, onHidden };
}
