import { useEffect, useRef } from "react";

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
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => document.removeEventListener("visibilitychange", onVisibilityChange);
  }, [active, reconcile]);
}
