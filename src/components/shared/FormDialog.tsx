import { useEffect, useRef, type FormEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { trapDialogTab } from "./dialogFocusTrap";

function useFormDialogFocus() {
  const dialog = useRef<HTMLFormElement>(null);
  useEffect(() => {
    const previous = document.activeElement;
    if (!dialog.current?.contains(previous)) {
      dialog.current?.querySelector<HTMLElement>("input:not([disabled]), button:not([disabled]), textarea:not([disabled])")?.focus();
    }
    return () => { if (previous instanceof HTMLElement && previous.isConnected) previous.focus(); };
  }, []);
  return dialog;
}

export function FormDialog({ titleId, descriptionId, onCancel, onSubmit, children, className = "max-w-sm" }: {
  titleId: string;
  descriptionId?: string;
  onCancel: () => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  children: ReactNode;
  className?: string;
}) {
  const dialog = useFormDialogFocus();
  const cancel = useRef(onCancel);
  cancel.current = onCancel;
  return createPortal(
    <div className="fixed inset-0 z-[1000] grid place-items-center bg-[color-mix(in_srgb,var(--ct-bg)_60%,transparent)] p-4"
      onMouseDown={(event) => { if (event.target === event.currentTarget) cancel.current(); }}>
      <form ref={dialog} role="dialog" aria-modal="true" aria-labelledby={titleId} aria-describedby={descriptionId}
        className={`flex w-full ${className} max-h-[calc(100dvh-2rem)] flex-col gap-3 overflow-auto rounded-lg border border-[var(--ct-border)] bg-[var(--ct-surface)] p-4 text-[var(--ct-fg)] shadow-xl`}
        onSubmit={onSubmit}
        onKeyDown={(event) => {
          event.stopPropagation();
          if (event.code === "Escape" || event.key === "Escape") { event.preventDefault(); cancel.current(); return; }
          trapDialogTab(event, dialog.current);
        }}>
        {children}
      </form>
    </div>, document.body,
  );
}
