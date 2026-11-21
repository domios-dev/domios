import { useEffect, useLayoutEffect, useRef, useState, type FormEvent, type CSSProperties, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import { trapDialogTab, useDialogFocus } from "./dialogFocusTrap";
import { useBackDismissal } from "../../lib/backDismissal";

/** The same backdrop as Start a session, so content behind a dialog never competes with it. */
const SCRIM = "bg-black/55 backdrop-blur-sm";
/** Over another dialog: a full scrim, so the parent's controls never read through. */
const STACKED_SCRIM = "bg-[color-mix(in_srgb,var(--ct-bg)_88%,transparent)] backdrop-blur-sm";

/**
 * Opens centred, then keeps that top before the first paint, so content that
 * appears later (an effort row, an error) grows the dialog downward instead
 * of re-centring it. A viewport resize re-centres once.
 */
function useStableTop(dialog: RefObject<HTMLFormElement | null>, enabled: boolean): number | null {
  const [top, setTop] = useState<number | null>(null);
  useLayoutEffect(() => {
    const form = dialog.current;
    const host = form?.parentElement;
    if (!enabled || top !== null || !form || !host) return;
    const hostPadding = Number.parseFloat(host.ownerDocument.defaultView?.getComputedStyle(host).paddingTop ?? "") || 0;
    setTop(Math.max(0, form.getBoundingClientRect().top - host.getBoundingClientRect().top - hostPadding));
  }, [dialog, enabled, top]);
  useEffect(() => {
    if (!enabled) return;
    let size = `${window.innerWidth}x${window.innerHeight}`;
    const recentre = () => {
      const next = `${window.innerWidth}x${window.innerHeight}`;
      if (next === size) return;
      size = next;
      setTop(null);
    };
    window.addEventListener("resize", recentre);
    return () => window.removeEventListener("resize", recentre);
  }, [enabled]);
  return enabled ? top : null;
}

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
  const explicitlyAnchored = style?.bottom !== undefined || style?.top !== undefined;
  const top = useStableTop(dialog, !fullscreen && !explicitlyAnchored);
  const anchored = top === null ? style : { ...style, marginTop: top, maxHeight: `calc(100dvh - ${top}px - 2rem)` };
  return createPortal(
    <div className={`fixed inset-0 z-[1000] grid ${top === null ? "place-items-center" : "items-start justify-items-center"} ${stacked ? STACKED_SCRIM : SCRIM} ${fullscreen ? "" : "p-4"}`} data-stacked={stacked || undefined}
      onMouseDown={(event) => { if (event.target === event.currentTarget) cancel.current(); }}>
      <form style={anchored} ref={dialog} role="dialog" aria-modal="true" aria-labelledby={titleId} aria-describedby={descriptionId}
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
