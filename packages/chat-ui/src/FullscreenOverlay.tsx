import { useEffect, type ReactNode, type RefObject, type CSSProperties } from "react";

export function useFullscreenKeyboard(open: boolean, ref: RefObject<HTMLDivElement | null>, close: () => void) {
  useEffect(() => {
    if (!open) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    ref.current?.querySelector<HTMLElement>("button")?.focus();
    const handle = (event: KeyboardEvent) => {
      const focusedDialog = document.activeElement?.closest('[role="dialog"][aria-modal="true"]');
      if (focusedDialog && focusedDialog !== ref.current) return;
      if (event.code === "Escape") {
        event.stopImmediatePropagation();
        event.preventDefault();
        close();
      } else if (event.code === "Tab") {
        const controls = Array.from(ref.current?.querySelectorAll<HTMLElement>('button, a[href], input, [tabindex="0"]') ?? []);
        const index = controls.indexOf(document.activeElement as HTMLElement);
        if (controls.length && (index < 0 || (!event.shiftKey && index === controls.length - 1) || (event.shiftKey && index === 0))) {
          event.preventDefault();
          controls[event.shiftKey ? controls.length - 1 : 0].focus();
        }
      }
    };
    window.addEventListener("keydown", handle, true);
    return () => {
      window.removeEventListener("keydown", handle, true);
      document.body.style.overflow = previous;
      if (opener?.isConnected) opener.focus();
    };
  }, [open, ref, close]);
}

export function FullscreenOverlay({ panelRef, label, style, onClose, children }: {
  panelRef: RefObject<HTMLDivElement | null>;
  label: string;
  style?: CSSProperties;
  onClose: () => void;
  children: ReactNode;
}) {
  return <div ref={panelRef} className="fixed z-50 bg-black/85 backdrop-blur-sm" style={style ?? { inset: "0px" }} onClick={onClose} role="dialog" aria-modal="true" aria-label={label}>
    <div className="mx-auto flex h-full w-full flex-col border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2 shadow-2xl" onClick={(event) => event.stopPropagation()}>{children}</div>
  </div>;
}
