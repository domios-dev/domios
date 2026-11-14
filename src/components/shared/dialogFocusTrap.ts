import { useEffect, useRef, type KeyboardEvent, type RefObject } from "react";

const FIRST_CONTROL = "input:not([disabled]), button:not([disabled]), textarea:not([disabled])";
const dialogStack: HTMLElement[] = [];

function ownsFocus(dialog: HTMLElement, active: Element | null): boolean {
  if (!active) return false;
  if (dialog.contains(active)) return true;
  return Array.from(dialog.querySelectorAll<HTMLElement>("[aria-controls]")).some((control) =>
    control.getAttribute("aria-controls")?.split(/\s+/).some((id) => dialog.ownerDocument.getElementById(id)?.contains(active)));
}

function isVisible(control: HTMLElement): boolean {
  for (let node: HTMLElement | null = control; node; node = node.parentElement) {
    const style = node.ownerDocument.defaultView?.getComputedStyle(node);
    if (node.hidden || node.inert || style?.display === "none" || style?.visibility === "hidden") return false;
  }
  return true;
}

/** A radio group is one tab stop on its checked radio (roving tabindex), so opening focus lands there too. */
function initialControl(dialog: HTMLElement | null): HTMLElement | null {
  const first = Array.from(dialog?.querySelectorAll<HTMLElement>(FIRST_CONTROL) ?? []).find(isVisible) ?? null;
  if (!(first instanceof HTMLInputElement) || first.type !== "radio" || !first.name) return first;
  const group = Array.from(dialog!.querySelectorAll<HTMLInputElement>('input[type="radio"]')).filter((radio) => radio.name === first.name);
  return group.find((radio) => radio.checked && !radio.disabled) ?? first;
}

/** Moves focus into a dialog when it opens and hands it back when it closes, to `restoreTo` when given, else to where it came from. */
export function useDialogFocus<T extends HTMLElement>(restoreTo?: () => HTMLElement | null | undefined): RefObject<T | null> {
  const dialog = useRef<T>(null);
  const restore = useRef(restoreTo);
  restore.current = restoreTo;
  useEffect(() => {
    const node = dialog.current;
    if (!node) return;
    const previous = document.activeElement;
    dialogStack.push(node);
    const recoverFocus = () => {
      if (dialogStack[dialogStack.length - 1] !== node || ownsFocus(node, document.activeElement)) return;
      initialControl(node)?.focus({ preventScroll: true });
    };
    recoverFocus();
    document.addEventListener("focusin", recoverFocus);
    const Observer = node.ownerDocument.defaultView?.MutationObserver;
    const observer = Observer ? new Observer(recoverFocus) : null;
    observer?.observe(node, { childList: true, subtree: true });
    return () => {
      observer?.disconnect();
      document.removeEventListener("focusin", recoverFocus);
      const index = dialogStack.indexOf(node);
      if (index >= 0) dialogStack.splice(index, 1);
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
  const focusable = Array.from(dialog?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? []).filter(isVisible);
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
