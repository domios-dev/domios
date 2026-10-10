import { useEffect, useRef, type RefObject } from "react";
import { createMessageLongPress } from "./messageLongPress";

const INTERACTIVE = "a, button, input, textarea, select, summary, label, [role='button'], [contenteditable='true'], [role='menu']";

export function useMessageTouch(row: RefObject<HTMLElement | null>, options: {
  selecting: boolean;
  open: (id: string | null) => void;
  toggle: (id: string) => void;
  range: (from: string, to: string, start: boolean) => void;
  endRange: () => void;
}) {
  const live = useRef(options);
  live.current = options;
  useEffect(() => {
    const element = row.current;
    if (!element) return;
    let targetId: string | null = null;
    let press: { id: number; x: number; y: number; gutter: boolean; dragging: boolean; selecting: boolean } | null = null;
    let suppressClick = false;
    let clearOpeningRelease: (() => void) | null = null;
    const guardOpeningRelease = () => {
      clearOpeningRelease?.();
      const mouse = (event: MouseEvent) => {
        if (event.type === "click" && event.detail === 0) return;
        event.preventDefault();
        event.stopImmediatePropagation();
        if (event.type === "click") clear();
      };
      const clear = () => {
        document.removeEventListener("mousedown", mouse, true);
        document.removeEventListener("mouseup", mouse, true);
        document.removeEventListener("click", mouse, true);
        document.removeEventListener("pointerdown", clear, true);
        clearOpeningRelease = null;
      };
      document.addEventListener("mousedown", mouse, true);
      document.addEventListener("mouseup", mouse, true);
      document.addEventListener("click", mouse, true);
      document.addEventListener("pointerdown", clear, true);
      clearOpeningRelease = clear;
    };
    const identify = (target: EventTarget | null, y: number) => {
      if (!(target instanceof Element)) return null;
      const part = target.closest<HTMLElement>("[data-message-part-id]") ?? Array.from(element.querySelectorAll<HTMLElement>("[data-message-part-id]")).find((part) => {
        const bounds = part.getBoundingClientRect();
        return y >= bounds.top && y <= bounds.bottom;
      });
      return part?.dataset.messagePartId ?? element.dataset.messageId ?? null;
    };
    const controller = createMessageLongPress({
      schedule: (callback, delay) => window.setTimeout(callback, delay),
      cancel: (handle) => window.clearTimeout(handle as number),
      owned: (owned) => element.toggleAttribute("data-touch-owned", owned),
      open: () => { suppressClick = true; guardOpeningRelease(); live.current.open(targetId); navigator.vibrate?.(10); },
    });
    const stop = () => {
      controller.cancel();
      if (press?.dragging) live.current.endRange();
      press = null;
      document.removeEventListener("pointermove", move);
      document.removeEventListener("pointerup", up);
      document.removeEventListener("pointercancel", stop);
      document.removeEventListener("scroll", stop, true);
    };
    const down = (event: PointerEvent) => {
      if (event.pointerType !== "touch") { suppressClick = false; return; }
      suppressClick = false;
      if (press || !event.isPrimary || !(event.target instanceof Element) || event.target.closest(INTERACTIVE)) { stop(); return; }
      targetId = identify(event.target, event.clientY);
      press = { id: event.pointerId, x: event.clientX, y: event.clientY, gutter: Boolean(event.target.closest("[data-message-gutter]")), dragging: false, selecting: live.current.selecting };
      document.addEventListener("pointermove", move, { passive: false });
      document.addEventListener("pointerup", up);
      document.addEventListener("pointercancel", stop);
      document.addEventListener("scroll", stop, true);
      if (!press.selecting) controller.start({ pointerId: event.pointerId, x: event.clientX, y: event.clientY });
    };
    const move = (event: PointerEvent) => {
      if (!press || press.id !== event.pointerId) return;
      controller.move({ pointerId: event.pointerId, x: event.clientX, y: event.clientY });
      if (Math.hypot(event.clientX - press.x, event.clientY - press.y) <= 8 && !press.dragging) return;
      if (!press.gutter || !press.selecting || !targetId) { stop(); return; }
      event.preventDefault();
      suppressClick = true;
      const over = document.elementFromPoint(event.clientX, event.clientY);
      const surface = element.closest("[role='log']");
      const rowAt = over?.closest<HTMLElement>("[data-message-id]");
      if (!rowAt || !surface?.contains(rowAt)) return;
      const part = over?.closest<HTMLElement>("[data-message-part-id]") ?? Array.from(rowAt.querySelectorAll<HTMLElement>("[data-message-part-id]")).find((part) => {
        const rect = part.getBoundingClientRect();
        return event.clientY >= rect.top && event.clientY <= rect.bottom;
      });
      const id = part?.dataset.messagePartId ?? rowAt.dataset.messageId;
      if (id) { live.current.range(targetId, id, !press.dragging); press.dragging = true; }
    };
    const up = (event: PointerEvent) => {
      if (!press || press.id !== event.pointerId) return;
      const consumed = controller.end(event.pointerId);
      if (press.selecting && !press.dragging && targetId) { live.current.toggle(targetId); suppressClick = true; }
      if (consumed || suppressClick) event.preventDefault();
      stop();
    };
    const click = (event: Event) => {
      if (!suppressClick) return;
      suppressClick = false;
      event.preventDefault(); event.stopPropagation();
    };
    const mouseDown = (event: Event) => {
      if (suppressClick || press?.selecting) { event.preventDefault(); event.stopPropagation(); }
    };
    const context = (event: Event) => { if (element.hasAttribute("data-touch-owned")) event.preventDefault(); };
    element.addEventListener("pointerdown", down);
    element.addEventListener("click", click, true);
    element.addEventListener("mousedown", mouseDown, true);
    element.addEventListener("contextmenu", context);
    return () => {
      stop();
      clearOpeningRelease?.();
      element.removeEventListener("pointerdown", down);
      element.removeEventListener("click", click, true);
      element.removeEventListener("mousedown", mouseDown, true);
      element.removeEventListener("contextmenu", context);
    };
  }, [row]);
}
