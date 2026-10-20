import { useCallback, useEffect, useRef, useState } from "react";

export function useRuntimeStatusPopover() {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (open) popoverRef.current?.focus({ preventScroll: true });
  }, [open]);
  const close = useCallback(() => setOpen(false), []);
  const toggle = useCallback(() => setOpen((value) => !value), []);
  return { open, triggerRef, popoverRef, close, toggle };
}
