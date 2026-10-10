import type { KeyboardEvent } from "react";

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
