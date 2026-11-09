import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { browserViewerTarget, ownsViewport } from "./viewportOwnerModel";
import { createViewportResizer } from "./viewportResizer";
import { publishNotice } from "../../lib/notices";
import { createFrameFeed, type FrameFeed } from "./frameFeed";
import { type StreamViewport } from "./streamModel";
import { streamLifecycleAction } from "./streamLifecycle";
import type { SurfaceState } from "./surfaceChromeModel";
import { isPreviewPaused, NOTICE_KIND, resizeArgs, SURFACE_METHOD, type Disconnect, type StreamFrame, type SurfaceNotice, type SurfaceStreamTransport } from "./surfaceStreamTransport";

export interface SurfaceStream {
  state: SurfaceState;
  notice: SurfaceNotice | null;
  /** Set by a preview_paused notice and cleared by the next frame. */
  paused: SurfaceNotice | null;
  unavailable: SurfaceNotice | null;
  frames: FrameFeed;
  setNotice(notice: SurfaceNotice | null): void;
  onViewport(viewport: StreamViewport): void;
  onHidden(): void;
  onInteraction(): void;
}

interface Connection {
  disconnect: Disconnect | null;
  sent: StreamViewport | null;
  latest: StreamViewport | null;
}

export function useSurfaceStream(transport: SurfaceStreamTransport, initialState: SurfaceState, visible = true): SurfaceStream {
  const [state, setState] = useState<SurfaceState>(initialState);
  const [notice, setNotice] = useState<SurfaceNotice | null>(null);
  const [paused, setPaused] = useState<SurfaceNotice | null>(null);
  const [unavailable, setUnavailable] = useState<SurfaceNotice | null>(null);
  const unavailableRef = useRef(false);
  const pausedRef = useRef(false);
  const connection = useRef<Connection>({ disconnect: null, sent: null, latest: null });
  const [viewer] = useState(() => crypto.randomUUID());
  const owner = useRef(state.viewportOwner);
  owner.current = state.viewportOwner;
  const [feed] = useState(() => createFrameFeed());

  const resizer = useMemo<{ current: ReturnType<typeof createViewportResizer> | null }>(() => ({ current: null }), [transport]);
  useEffect(() => () => { resizer.current?.dispose(); resizer.current = null; }, [resizer]);

  const sendResize = useCallback((viewport: StreamViewport) => {
    connection.current.sent = viewport;
    resizer.current ??= createViewportResizer(
      (next) => ownsViewport(viewer, owner.current)
        ? transport.invoke(SURFACE_METHOD.resize, { ...resizeArgs(next), viewer })
        : Promise.resolve(),
      (error) => publishNotice({ kind: "debug", severity: "Warning", title: "Background surface resize failed", detail: String(error) }),
    );
    resizer.current.push(viewport);
  }, [resizer, transport, viewer]);

  const connect = useCallback((viewport: StreamViewport) => {
    const current = connection.current;
    current.sent = viewport;
    current.disconnect = transport.connect(viewport, {
      frame: (frame: StreamFrame) => {
        feed.publish(frame);
        if (unavailableRef.current) {
          unavailableRef.current = false;
          setUnavailable(null);
        }
        if (pausedRef.current) {
          pausedRef.current = false;
          setPaused(null);
        }
      },
      state: setState,
      notice: (next: SurfaceNotice) => {
        if (next.kind === NOTICE_KIND.unavailable) {
          unavailableRef.current = true;
          setUnavailable(next);
        }
        if (!isPreviewPaused(next)) {
          setNotice(next);
          return;
        }
        pausedRef.current = true;
        setPaused(next);
      },
      open: () => {
        resizer.current?.dispose();
        resizer.current = null;
        if (current.latest && ownsViewport(viewer, owner.current)) sendResize(current.latest);
      },
    }, viewer, browserViewerTarget());
  }, [transport, feed, viewer, resizer, sendResize]);

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
      current.latest = null;
    } else if (current.latest && !current.disconnect) {
      connect(current.latest);
    }
  }, [connect, visible]);

  const onViewport = useCallback((viewport: StreamViewport) => {
    const current = connection.current;
    current.latest = viewport;
    const action = streamLifecycleAction(visible, current.disconnect !== null, current.sent, viewport);
    if (action === "connect") connect(viewport);
    else if (action === "resize" && ownsViewport(viewer, owner.current)) sendResize(viewport);
    else if (action === "disconnect") {
      current.disconnect?.();
      current.disconnect = null;
      current.sent = null;
    }
  }, [connect, sendResize, visible, viewer]);

  useEffect(() => {
    const current = connection.current;
    if (visible && ownsViewport(viewer, state.viewportOwner) && current.disconnect && current.latest
      && streamLifecycleAction(true, true, current.sent, current.latest) === "resize") {
      sendResize(current.latest);
    }
  }, [visible, viewer, state.viewportOwner, sendResize]);

  const onHidden = useCallback(() => {
    const current = connection.current;
    current.disconnect?.();
    current.disconnect = null;
    current.sent = null;
  }, []);

  const onInteraction = useCallback(() => {
    const current = connection.current;
    if (!visible || !current.disconnect || !current.latest || ownsViewport(viewer, owner.current)) return;
    void transport.invoke("viewport.claim", { ...resizeArgs(current.latest), viewer })
      .catch((error: unknown) => publishNotice({ kind: "debug", severity: "Warning", title: "Surface viewport claim failed", detail: String(error) }));
  }, [transport, viewer, visible]);

  return { state, notice, paused, unavailable, frames: feed, setNotice, onViewport, onHidden, onInteraction };
}
