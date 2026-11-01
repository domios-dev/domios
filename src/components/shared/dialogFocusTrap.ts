import { useEffect, useRef, type KeyboardEvent, type RefObject } from "react";

const FIRST_CONTROL = "input:not([disabled]), button:not([disabled]), textarea:not([disabled])";

/** Moves focus into a dialog when it opens and hands it back when it closes, to `restoreTo` when given, else to where it came from. */
export function useDialogFocus<T extends HTMLElement>(restoreTo?: () => HTMLElement | null | undefined): RefObject<T | null> {
  const dialog = useRef<T>(null);
  const restore = useRef(restoreTo);
  restore.current = restoreTo;
  useEffect(() => {
    const previous = document.activeElement;
    if (!dialog.current?.contains(previous)) dialog.current?.querySelector<HTMLElement>(FIRST_CONTROL)?.focus();
    return () => {
      const target = restore.current?.() ?? previous;
      if (target instanceof HTMLElement && target.isConnected) target.focus();
    };
  }, []);
  return dialog;
}

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function trapDialogTab(event: KeyboardEvent<HTMLElement>, dialog: HTMLElement | null): void {
  if (event.key !== "Tab") return;
  const focusable = Array.from(dialog?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? []);
  if (focusable.length === 0) {
    event.preventDefault();
    return;
  }
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  const active = document.activeElement;
  if (event.shiftKey && (active === first || !dialog?.contains(active))) {
    event.preventDefault();
    last?.focus();
  } else if (!event.shiftKey && (active === last || !dialog?.contains(active))) {
    event.preventDefault();
    first?.focus();
  }
}
