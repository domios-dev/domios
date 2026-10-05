import { useEffect, useId, useRef } from "react";
import { AUTOMATION_TOOL } from "../Automation/styles";

export function JobConfirmationDialog({ title, description, confirmLabel, busy, error, onCancel, onConfirm }: {
  title: string;
  description: string;
  confirmLabel: string;
  busy: boolean;
  error: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const id = useId();
  const dialog = useRef<HTMLDivElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const actions = useRef({ onCancel, busy });
  actions.current = { onCancel, busy };

  useEffect(() => {
    cancel.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (!actions.current.busy) actions.current.onCancel();
        return;
      }
      if (event.key !== "Tab") return;
      event.preventDefault();
      if (event.shiftKey) {
        if (document.activeElement === cancel.current) dialog.current?.querySelector<HTMLButtonElement>("[data-dialog-confirm]")?.focus();
        else cancel.current?.focus();
      } else if (document.activeElement === dialog.current?.querySelector("[data-dialog-confirm]")) {
        cancel.current?.focus();
      } else {
        dialog.current?.querySelector<HTMLButtonElement>("[data-dialog-confirm]")?.focus();
      }
    };
    const onFocusIn = (event: FocusEvent) => {
      if (dialog.current && !dialog.current.contains(event.target as Node)) cancel.current?.focus();
    };
    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("focusin", onFocusIn, true);
    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
      document.removeEventListener("focusin", onFocusIn, true);
    };
  }, []);

  return <div className="fixed inset-0 z-[1000] grid place-items-center bg-[color-mix(in_srgb,var(--ct-bg)_72%,transparent)] p-4">
    <div ref={dialog} role="alertdialog" aria-modal="true" aria-labelledby={`${id}-title`} aria-describedby={`${id}-description`} className="w-full max-w-md rounded-lg border border-[var(--ct-border)] bg-[var(--ct-bg)] p-5 text-[var(--ct-fg)] shadow-xl">
      <h2 id={`${id}-title`} className="text-[length:var(--ct-font-size-md)] font-semibold">{title}</h2>
      <p id={`${id}-description`} className="mt-2 text-sm text-[var(--ct-muted-fg)]">{description}</p>
      {error && <p role="alert" className="mt-3 text-sm text-[var(--ct-fg)]">{error}</p>}
      <div className="mt-5 flex justify-end gap-2">
        <button ref={cancel} type="button" className={AUTOMATION_TOOL} disabled={busy} onClick={onCancel}>Cancel</button>
        <button type="button" data-dialog-confirm className={AUTOMATION_TOOL} disabled={busy} onClick={onConfirm}>
          {busy ? "Working…" : confirmLabel}
        </button>
      </div>
    </div>
  </div>;
}
