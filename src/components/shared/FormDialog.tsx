import { useRef, type FormEvent, type CSSProperties, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { trapDialogTab, useDialogFocus } from "./dialogFocusTrap";
import { useBackDismissal } from "../../lib/backDismissal";

const SCRIM = "bg-[color-mix(in_srgb,var(--ct-bg)_60%,transparent)]";
/** Over another dialog: a full scrim, so the parent's controls never read through. */
const STACKED_SCRIM = "bg-[color-mix(in_srgb,var(--ct-bg)_88%,transparent)] backdrop-blur-sm";

export function FormDialog({ titleId, descriptionId, onCancel, onSubmit, children, style, className = "max-w-sm", stacked = false, fullscreen = false }: {
  titleId: string;
  descriptionId?: string;
  onCancel: () => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  stacked?: boolean;
  fullscreen?: boolean;
}) {
  const dialog = useDialogFocus<HTMLFormElement>();
  const cancel = useRef(onCancel);
  cancel.current = onCancel;
  useBackDismissal(true, onCancel);
  return createPortal(
    <div className={`fixed inset-0 z-[1000] grid place-items-center ${stacked ? STACKED_SCRIM : SCRIM} ${fullscreen ? "" : "p-4"}`} data-stacked={stacked || undefined}
      onMouseDown={(event) => { if (event.target === event.currentTarget) cancel.current(); }}>
      <form style={style} ref={dialog} role="dialog" aria-modal="true" aria-labelledby={titleId} aria-describedby={descriptionId}
        className={`flex w-full ${className} ${fullscreen ? "h-[100dvh] overflow-hidden" : "max-h-[calc(100dvh-2rem)] overflow-auto rounded-lg border border-[var(--ct-border)]"} flex-col gap-3 bg-[var(--ct-surface)] p-4 text-[var(--ct-fg)] shadow-xl`}
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
