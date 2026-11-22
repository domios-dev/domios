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
    let blurred = !document.hasFocus();
    const onVisibilityChange = () => {
      const wasHidden = hidden;
      hidden = document.visibilityState === "hidden";
      if (wasHidden && !hidden) {
        blurred = false;
        reconcile("window_activated");
      }
    };
    const onBlur = () => {
      blurred = true;
    };
    // On Windows WebView2, covering Domios with another window does not toggle visibilityState.
    const onFocus = () => {
      if (!blurred || document.visibilityState === "hidden") return;
      blurred = false;
      reconcile("window_focused");
    };
    // A read issued on resume can die with a stalled carrier; the reconnected stream catches the transcript up.
    const onStreamReconnected = () => reconcile("stream_reconnected");
    document.addEventListener("visibilitychange", onVisibilityChange);
    window.addEventListener("blur", onBlur);
    window.addEventListener("focus", onFocus);
    window.addEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, onStreamReconnected);
    return () => {
      document.removeEventListener("visibilitychange", onVisibilityChange);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, onStreamReconnected);
    };
  }, [active, reconcile]);
}
