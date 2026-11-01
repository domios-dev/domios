import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { createContext, useContext, useEffect, useRef, useState, type ReactNode, type RefObject } from "react";

let fullscreenDepth = 0;
let previousBodyOverflow = "";
const FullscreenViewerContext = createContext<RefObject<HTMLDivElement | null> | null>(null);

export function useFullscreenViewer() {
  return useContext(FullscreenViewerContext);
}

export function useFullscreenKeyboard(open: boolean, ref: RefObject<HTMLDivElement | null>, close: () => void, returnFocusRef?: RefObject<HTMLElement | null>, dialogRef?: RefObject<HTMLDialogElement | null>) {
  const closeRef = useRef(close);
  closeRef.current = close;
  useEffect(() => {
    if (!open) return;
    const active = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const opener = active && active !== document.body ? active : returnFocusRef?.current?.querySelector<HTMLElement>("button") ?? returnFocusRef?.current ?? null;
    const dialog = dialogRef?.current;
    dialog?.showModal();
    if (fullscreenDepth === 0) previousBodyOverflow = document.body.style.overflow;
    fullscreenDepth += 1;
    document.body.style.overflow = "hidden";
    (ref.current?.querySelector<HTMLElement>("[data-fullscreen-autofocus]") ?? ref.current?.querySelector<HTMLElement>("button"))?.focus();
    const handle = (event: KeyboardEvent) => {
      if (dialog && (event.code === "Escape" || event.key === "Escape") && ref.current?.querySelector("[data-anchored-popover]")) return;
      const focusedDialog = document.activeElement?.closest('[role="dialog"][aria-modal="true"]');
      if (focusedDialog && focusedDialog !== ref.current) return;
      if (event.code === "Escape" || event.key === "Escape") {
        event.stopImmediatePropagation();
        event.preventDefault();
        closeRef.current();
      } else if (event.code === "Tab" || event.key === "Tab") {
        const controls = Array.from(ref.current?.querySelectorAll<HTMLElement>('button:not([disabled]), a[href], input:not([disabled]), textarea, select, iframe, video[controls], [tabindex="0"]') ?? []).filter(control => !dialog || !control.closest("[hidden]"));
        const index = controls.indexOf(document.activeElement as HTMLElement);
        if (controls.length && (index < 0 || (!event.shiftKey && index === controls.length - 1) || (event.shiftKey && index === 0))) {
          event.preventDefault();
          controls[event.shiftKey ? controls.length - 1 : 0].focus();
        }
      }
    };
    const fromFrame = (event: MessageEvent) => {
      if (event.data?.source !== "ct-html-preview" || event.data?.action !== "close") return;
      const source = event.source;
      if (source === null) return;
      if (Array.from(ref.current?.querySelectorAll("iframe") ?? []).some((frame) => frame.contentWindow !== null && frame.contentWindow === source)) closeRef.current();
    };
    window.addEventListener("keydown", handle, true);
    window.addEventListener("message", fromFrame);
    return () => {
      window.removeEventListener("keydown", handle, true);
      window.removeEventListener("message", fromFrame);
      fullscreenDepth -= 1;
      if (fullscreenDepth === 0) document.body.style.overflow = previousBodyOverflow;
      dialog?.close();
      if (opener?.isConnected) opener.focus({ preventScroll: true });
    };
  }, [open, ref, returnFocusRef, dialogRef]);
}

export function FullscreenOverlay({ panelRef, label, onClose, children, returnFocusRef, closeButton = true, open, header }: {
  panelRef: RefObject<HTMLDivElement | null>;
  closeButton?: boolean;
  label: string;
  returnFocusRef?: RefObject<HTMLElement | null>;
  onClose: () => void;
  children: ReactNode;
  open?: boolean;
  header?: ReactNode;
}) {
  const viewer = useFullscreenViewer();
  const [portalHost] = useState(() => viewer?.current ?? document.body);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const expanded = open ?? true;
  useFullscreenKeyboard(expanded, panelRef, onClose, returnFocusRef, open === undefined ? undefined : dialogRef);
  const content = (
    <FullscreenViewerContext.Provider value={open !== undefined && expanded ? panelRef : viewer}>
    <div ref={panelRef} className={expanded ? "ct-chat-fullscreen fixed z-[60] bg-black/85 backdrop-blur-sm" : undefined} style={expanded ? { inset: 0, padding: "max(8px, env(safe-area-inset-top)) 8px max(8px, env(safe-area-inset-bottom))" } : { display: "contents" }} onClick={expanded ? (event) => { if (open === undefined || event.target === event.currentTarget) onClose(); } : undefined} role={expanded ? "dialog" : undefined} aria-modal={expanded ? true : undefined} aria-label={expanded ? label : undefined}>
      <div className={expanded ? "relative mx-auto flex h-full w-full flex-col border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2 shadow-2xl" : undefined} style={expanded ? undefined : { display: "contents" }} onClick={(event) => event.stopPropagation()}>
        {(header || (expanded && closeButton)) && <div className={header ? "flex shrink-0 items-center gap-2" : "flex shrink-0 justify-end pb-1"}>
          {header && <div className="min-w-0 flex-1">{header}</div>}
          {expanded && closeButton && <button type="button" aria-label="Close fullscreen viewer" data-fullscreen-autofocus={open !== undefined ? true : undefined} onClick={onClose} style={{ minWidth: 44, minHeight: 44, flexShrink: 0, background: "var(--ct-bg)", color: "var(--ct-fg)", border: "1px solid var(--ct-border)", borderRadius: 8, display: "grid", placeItems: "center" }}><X size={20} aria-hidden /></button>}
        </div>}
        <div className={expanded ? "flex min-h-0 flex-1 flex-col" : undefined} style={expanded ? undefined : { display: "contents" }}>{children}</div>
      </div>
    </div>
    </FullscreenViewerContext.Provider>
  );
  if (open === undefined) return createPortal(content, portalHost);
  return <dialog ref={dialogRef} role="presentation" style={{ display: expanded ? "block" : "contents", position: "fixed", inset: 0, width: "100%", height: "100%", maxWidth: "none", maxHeight: "none", overflow: "visible", padding: 0, border: 0, margin: 0, color: "inherit", background: "transparent" }} onCancel={(event) => { event.preventDefault(); onClose(); }}>{content}</dialog>;
}
