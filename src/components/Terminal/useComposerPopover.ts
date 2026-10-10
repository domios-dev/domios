import { useLayoutEffect, useRef, useState } from "react";

export function useComposerPopover() {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (!open) return;
    const first = popoverRef.current?.querySelector<HTMLElement>('input:checked:not(:disabled), input:not(:disabled), button:not(:disabled)');
    first?.focus({ preventScroll: true });
  }, [open]);
  const close = () => { setOpen(false); triggerRef.current?.focus({ preventScroll: true }); };
  return { open, triggerRef, popoverRef, close, toggle() { if (open) close(); else setOpen(true); } };
}
