import { useCallback, useEffect, useRef, useState } from "react";
import { createFrameFeed, type FrameFeed } from "./frameFeed";
import { viewportAction, type StreamViewport } from "./streamModel";
import type { SurfaceState } from "./surfaceChromeModel";
import { resizeArgs, SURFACE_METHOD, type Disconnect, type SurfaceNotice, type SurfaceStreamTransport } from "./surfaceStreamTransport";

export interface SurfaceStream {
  state: SurfaceState;
  notice: SurfaceNotice | null;
  frames: FrameFeed;
  setNotice(notice: SurfaceNotice | null): void;
  onViewport(viewport: StreamViewport): void;
}

interface Connection {
  disconnect: Disconnect | null;
  sent: StreamViewport | null;
  latest: StreamViewport | null;
}

export function useSurfaceStream(transport: SurfaceStreamTransport, initialState: SurfaceState): SurfaceStream {
  const [state, setState] = useState<SurfaceState>(initialState);
  const [notice, setNotice] = useState<SurfaceNotice | null>(null);
  const [feed] = useState(createFrameFeed);
  const connection = useRef<Connection>({ disconnect: null, sent: null, latest: null });

  const sendResize = useCallback((viewport: StreamViewport) => {
    connection.current.sent = viewport;
    transport.invoke(SURFACE_METHOD.resize, resizeArgs(viewport)).catch((error: unknown) => {
      setNotice({ text: error instanceof Error ? error.message : String(error), tone: "error" });
    });
  }, [transport]);

  const connect = useCallback((viewport: StreamViewport) => {
    const current = connection.current;
    current.sent = viewport;
    current.disconnect = transport.connect(viewport, {
      frame: feed.publish,
      state: setState,
      notice: setNotice,
      // A reconnect reuses the original connect params, so the current size is re-sent on every open.
      open: () => { if (current.latest) sendResize(current.latest); },
    });
  }, [transport, feed, sendResize]);

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

  return { state, notice, frames: feed, setNotice, onViewport };
}
