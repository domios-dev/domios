import { useCallback, useEffect, useRef, useState } from "react";
import { createFrameFeed, type FrameFeed } from "./frameFeed";
import { frameMatchesViewport, type FrameMetadata, type StreamViewport } from "./streamModel";
import { streamLifecycleAction } from "./streamLifecycle";
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
  mismatch: string | null;
}

// Only the visible, focused window may claim the viewport every viewer shares.
function documentActive(): boolean {
  if (typeof document === "undefined") return false;
  const focused = typeof document.hasFocus === "function" ? document.hasFocus() : true;
  return document.visibilityState === "visible" && focused;
}

export function useSurfaceStream(transport: SurfaceStreamTransport, initialState: SurfaceState, visible = true): SurfaceStream {
  const [state, setState] = useState<SurfaceState>(initialState);
  const [notice, setNotice] = useState<SurfaceNotice | null>(null);
  const [paused, setPaused] = useState<SurfaceNotice | null>(null);
  const pausedRef = useRef(false);
  const [feed] = useState(createFrameFeed);
  const connection = useRef<Connection>({ disconnect: null, sent: null, latest: null, mismatch: null });

  const sendResize = useCallback((viewport: StreamViewport) => {
    connection.current.sent = viewport;
    transport.invoke(SURFACE_METHOD.resize, resizeArgs(viewport)).catch((error: unknown) => {
      setNotice({ text: error instanceof Error ? error.message : String(error), tone: "error" });
    });
  }, [transport]);

  const heal = useCallback((meta: FrameMetadata) => {
    const current = connection.current;
    // sent is null while the stage is hidden; a hidden stage must not claim the shared viewport.
    if (!current.latest || !current.sent || !documentActive()) return;
    if (frameMatchesViewport(meta, current.latest)) {
      current.mismatch = null;
      return;
    }
    const mismatch = `${meta.deviceWidth}:${meta.deviceHeight}:${current.latest.w}:${current.latest.h}:${current.latest.dpr}`;
    if (current.mismatch === mismatch) return;
    current.mismatch = mismatch;
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
    return () => {
      current.disconnect?.();
      current.disconnect = null;
      current.sent = null;
    };
  }, [connect]);

  useEffect(() => {
    const current = connection.current;
    if (!visible) {
      current.disconnect?.();
      current.disconnect = null;
      current.sent = null;
    } else if (current.latest && !current.disconnect) {
      connect(current.latest);
    }
  }, [connect, visible]);

  const onViewport = useCallback((viewport: StreamViewport) => {
    const current = connection.current;
    current.latest = viewport;
    const action = streamLifecycleAction(visible, current.disconnect !== null, current.sent, viewport);
    if (action === "connect") connect(viewport);
    else if (action === "resize") sendResize(viewport);
    else if (action === "disconnect") {
      current.disconnect?.();
      current.disconnect = null;
      current.sent = null;
    }
  }, [connect, sendResize, visible]);

  const onHidden = useCallback(() => {
    const current = connection.current;
    current.disconnect?.();
    current.disconnect = null;
    current.sent = null;
  }, []);

  return { state, notice, paused, frames: feed, setNotice, onViewport, onHidden };
}
