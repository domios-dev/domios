import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode, type RefObject } from "react";

let fullscreenDepth = 0;
let previousBodyOverflow = "";

export function useFullscreenKeyboard(open: boolean, ref: RefObject<HTMLDivElement | null>, close: () => void, returnFocusRef?: RefObject<HTMLElement | null>) {
  const closeRef = useRef(close);
  closeRef.current = close;
  useEffect(() => {
    if (!open) return;
    const active = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const opener = active && active !== document.body ? active : returnFocusRef?.current?.querySelector<HTMLElement>("button") ?? returnFocusRef?.current ?? null;
    if (fullscreenDepth === 0) previousBodyOverflow = document.body.style.overflow;
    fullscreenDepth += 1;
    document.body.style.overflow = "hidden";
    (ref.current?.querySelector<HTMLElement>("[data-fullscreen-autofocus]") ?? ref.current?.querySelector<HTMLElement>("button"))?.focus();
    const handle = (event: KeyboardEvent) => {
      const focusedDialog = document.activeElement?.closest('[role="dialog"][aria-modal="true"]');
      if (focusedDialog && focusedDialog !== ref.current) return;
      if (event.code === "Escape" || event.key === "Escape") {
        event.stopImmediatePropagation();
        event.preventDefault();
        closeRef.current();
      } else if (event.code === "Tab" || event.key === "Tab") {
        const controls = Array.from(ref.current?.querySelectorAll<HTMLElement>('button:not([disabled]), a[href], input:not([disabled]), textarea, select, iframe, video[controls], [tabindex="0"]') ?? []);
        const index = controls.indexOf(document.activeElement as HTMLElement);
        if (controls.length && (index < 0 || (!event.shiftKey && index === controls.length - 1) || (event.shiftKey && index === 0))) {
          event.preventDefault();
          controls[event.shiftKey ? controls.length - 1 : 0].focus();
        }
      }
    };
    const fromFrame = (event: MessageEvent) => {
      if (event.data?.source !== "ct-html-preview" || event.data?.action !== "close") return;
      if (Array.from(ref.current?.querySelectorAll("iframe") ?? []).some((frame) => frame.contentWindow === event.source)) closeRef.current();
    };
    window.addEventListener("keydown", handle, true);
    window.addEventListener("message", fromFrame);
    return () => {
      window.removeEventListener("keydown", handle, true);
      window.removeEventListener("message", fromFrame);
      fullscreenDepth -= 1;
      if (fullscreenDepth === 0) document.body.style.overflow = previousBodyOverflow;
      if (opener?.isConnected) opener.focus({ preventScroll: true });
    };
  }, [open, ref, returnFocusRef]);
}

export function FullscreenOverlay({ panelRef, label, onClose, children, returnFocusRef, closeButton = true }: {
  panelRef: RefObject<HTMLDivElement | null>;
  closeButton?: boolean;
  label: string;
  returnFocusRef?: RefObject<HTMLElement | null>;
  onClose: () => void;
  children: ReactNode;
}) {
  const [portalHost] = useState(() => document.body);
  useFullscreenKeyboard(true, panelRef, onClose, returnFocusRef);
  return createPortal(
    <div ref={panelRef} className="ct-chat-fullscreen fixed z-[60] bg-black/85 backdrop-blur-sm" style={{ inset: 0, padding: "max(8px, env(safe-area-inset-top)) 8px max(8px, env(safe-area-inset-bottom))" }} onClick={onClose} role="dialog" aria-modal="true" aria-label={label}>
      <div className="relative mx-auto flex h-full w-full flex-col border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2 shadow-2xl" onClick={(event) => event.stopPropagation()}>
        {closeButton && <div className="flex shrink-0 justify-end pb-1">
          <button type="button" aria-label="Close fullscreen viewer" onClick={onClose} style={{ minWidth: 44, minHeight: 44, background: "var(--ct-bg)", color: "var(--ct-fg)", border: "1px solid var(--ct-border)", borderRadius: 8, display: "grid", placeItems: "center" }}><X size={20} aria-hidden /></button>
        </div>}
        <div className="flex min-h-0 flex-1 flex-col">{children}</div>
      </div>
    </div>, portalHost,
  );
}
