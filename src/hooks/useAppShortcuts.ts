import { useEffect, useRef } from "react";
import { dispatchAppShortcut, type AppShortcutBindings, type AppShortcutHandlers } from "../lib/appShortcuts";

/** One window-level dispatcher for every app shortcut. Capture phase, so a focused xterm cannot swallow the chord first. */
export function useAppShortcuts(bindings: AppShortcutBindings, handlers: AppShortcutHandlers): void {
  const latest = useRef({ bindings, handlers });
  latest.current = { bindings, handlers };
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.isComposing) return;
      dispatchAppShortcut(event, latest.current.bindings, latest.current.handlers);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);
}
