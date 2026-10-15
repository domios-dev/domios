import { useEffect, useRef, useState } from "react";

export function useRuntimeStatusPopover() {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    popoverRef.current?.focus({ preventScroll: true });
    const dismissOutside = (event: PointerEvent) => {
      if (event.target instanceof Node && !triggerRef.current?.contains(event.target)
        && !popoverRef.current?.contains(event.target)) setOpen(false);
    };
    const dismissEscape = (event: KeyboardEvent) => {
      if (event.code !== "Escape") return;
      event.preventDefault();
      setOpen(false);
      triggerRef.current?.focus({ preventScroll: true });
    };
    document.addEventListener("pointerdown", dismissOutside);
    document.addEventListener("keydown", dismissEscape);
    return () => {
      document.removeEventListener("pointerdown", dismissOutside);
      document.removeEventListener("keydown", dismissEscape);
    };
  }, [open]);
  return { open, triggerRef, popoverRef, toggle: () => setOpen(value => !value) };
}
