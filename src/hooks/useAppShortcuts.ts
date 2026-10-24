import { useEffect, useRef } from "react";
import { dispatchAppShortcut, type AppShortcutBindings, type AppShortcutHandlers } from "../lib/appShortcuts";

/** One window-level dispatcher for every app shortcut. Capture phase, so a focused xterm cannot swallow the chord first. */
export function useAppShortcuts(bindings: AppShortcutBindings, handlers: AppShortcutHandlers, disabled = false): void {
  const latest = useRef({ bindings, handlers, disabled });
  latest.current = { bindings, handlers, disabled };
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.isComposing) return;
      const current = latest.current;
      const handlers = current.disabled
        ? Object.fromEntries(Object.keys(current.handlers).map((action) => [action, () => {}])) as AppShortcutHandlers
        : current.handlers;
      dispatchAppShortcut(event, current.bindings, handlers);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);
}
