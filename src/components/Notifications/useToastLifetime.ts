import { useEffect, useRef, useState } from "react";
import { remainingLifetime, toastLifetime } from "./toastLifetime";

export function useToastLifetime(id: string, configured: number | undefined, onDismiss: (id: string) => void) {
  const [duration] = useState(() => toastLifetime(configured));
  const [paused, setPaused] = useState(false);
  const remaining = useRef(duration);
  const dismissed = useRef(false);
  const dismissHandler = useRef(onDismiss);
  dismissHandler.current = onDismiss;
  const dismiss = () => {
    if (dismissed.current) return;
    dismissed.current = true;
    dismissHandler.current(id);
  };
  const dismissRef = useRef(dismiss);
  dismissRef.current = dismiss;

  useEffect(() => {
    if (paused || dismissed.current) return;
    const startedAt = Date.now();
    // This deadline expires content; it never coordinates readiness.
    const deadline = window.setTimeout(() => dismissRef.current(), remaining.current);
    const checkVisibility = () => {
      if (remainingLifetime(remaining.current, startedAt, Date.now()) === 0) dismissRef.current();
    };
    document.addEventListener("visibilitychange", checkVisibility);
    return () => {
      window.clearTimeout(deadline);
      document.removeEventListener("visibilitychange", checkVisibility);
      remaining.current = remainingLifetime(remaining.current, startedAt, Date.now());
    };
  }, [paused]);

  return { duration, paused, dismiss, pause: () => setPaused(true), resume: () => setPaused(false) };
}
