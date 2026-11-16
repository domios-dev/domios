import { useEffect, useRef } from "react";
import { TERMINAL_VIEWER_RECONNECTED_EVENT } from "../../lib/terminalViewerEvents";

export function useChatActivationReconcile(active: boolean, reconcile: (cause: string) => void): void {
  const wasActive = useRef(active);
  useEffect(() => {
    const activated = active && !wasActive.current;
    wasActive.current = active;
    if (activated) reconcile("tab_activated");
  }, [active, reconcile]);

  useEffect(() => {
    if (!active) return;
    let hidden = document.visibilityState === "hidden";
    const onVisibilityChange = () => {
      const wasHidden = hidden;
      hidden = document.visibilityState === "hidden";
      if (wasHidden && !hidden) reconcile("window_activated");
    };
    // A read issued on resume can die with a stalled carrier; the reconnected stream catches the transcript up.
    const onStreamReconnected = () => reconcile("stream_reconnected");
    document.addEventListener("visibilitychange", onVisibilityChange);
    window.addEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, onStreamReconnected);
    return () => {
      document.removeEventListener("visibilitychange", onVisibilityChange);
      window.removeEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, onStreamReconnected);
    };
  }, [active, reconcile]);
}
