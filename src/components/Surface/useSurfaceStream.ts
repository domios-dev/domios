import { useCallback, useEffect, useRef, useState } from "react";
import { browserViewerTarget } from "./viewportOwnerModel";
import { publishNotice } from "../../lib/notices";
import { createFrameFeed, type FrameFeed } from "./frameFeed";
import { type StreamViewport } from "./streamModel";
import { streamLifecycleAction } from "./streamLifecycle";
import type { SurfaceState } from "./surfaceChromeModel";
import { isPreviewPaused, NOTICE_KIND, resizeArgs, type Disconnect, type StreamFrame, type SurfaceNotice, type SurfaceStreamTransport } from "./surfaceStreamTransport";

export interface SurfaceStream {
  state: SurfaceState;
  notice: SurfaceNotice | null;
  /** Set by a preview_paused notice and cleared by the next frame. */
  paused: SurfaceNotice | null;
  unavailable: SurfaceNotice | null;
  frames: FrameFeed;
  subscribeCursor(listener: (cursor: string) => void): () => void;
  setNotice(notice: SurfaceNotice | null): void;
  onViewport(viewport: StreamViewport): void;
  onHidden(): void;
  takeControl(): void;
  releaseControl(): void;
}

interface Connection {
  disconnect: Disconnect | null;
  sent: StreamViewport | null;
  latest: StreamViewport | null;
}

export function useSurfaceStream(transport: SurfaceStreamTransport, initialState: SurfaceState, visible = true, interactive = false): SurfaceStream {
  const [state, setState] = useState<SurfaceState>(initialState);
  const [notice, setNotice] = useState<SurfaceNotice | null>(null);
  const [paused, setPaused] = useState<SurfaceNotice | null>(null);
  const [unavailable, setUnavailable] = useState<SurfaceNotice | null>(null);
  const unavailableRef = useRef(false);
  const pausedRef = useRef(false);
  const interactiveRef = useRef(interactive);
  interactiveRef.current = interactive;
  const connection = useRef<Connection>({ disconnect: null, sent: null, latest: null });
  const [viewer] = useState(() => crypto.randomUUID());
  const [feed] = useState(() => createFrameFeed());
  const cursorState = useRef<{ value: string; listeners: Set<(cursor: string) => void> }>({ value: "default", listeners: new Set() });

  const subscribeCursor = useCallback((listener: (cursor: string) => void) => {
    const store = cursorState.current;
    store.listeners.add(listener);
    listener(store.value);
    return () => { store.listeners.delete(listener); };
  }, []);

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
      cursor: (next: string) => {
        const store = cursorState.current;
        if (store.value === next) return;
        store.value = next;
        for (const listener of store.listeners) listener(next);
      },
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
        if (interactiveRef.current) {
          void transport.invoke("control.claim", { viewer }).catch(() => undefined);
        }
      },
    }, viewer, browserViewerTarget(), interactiveRef.current);
  }, [transport, feed, viewer]);

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
    else if (action === "disconnect") {
      current.disconnect?.();
      current.disconnect = null;
      current.sent = null;
    }
  }, [connect, visible]);

  const onHidden = useCallback(() => {
    const current = connection.current;
    current.disconnect?.();
    current.disconnect = null;
    current.sent = null;
  }, []);

  const takeControl = useCallback(() => {
    const current = connection.current;
    if (!visible || !current.disconnect || !current.latest) return;
    void transport.invoke("viewport.claim", { ...resizeArgs(current.latest), viewer })
      .catch((error: unknown) => publishNotice({ kind: "debug", severity: "Warning", title: "Surface viewport claim failed", detail: String(error) }));
  }, [transport, viewer, visible]);

  const releaseControl = useCallback(() => {
    const current = connection.current;
    if (!current.disconnect) return;
    void transport.invoke("control.release", { viewer })
      .catch(() => undefined);
  }, [transport, viewer]);

  return { state, notice, paused, unavailable, frames: feed, subscribeCursor, setNotice, onViewport, onHidden, takeControl, releaseControl };
}
