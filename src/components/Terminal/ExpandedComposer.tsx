import { useEffect, useId, useLayoutEffect, useState, type CSSProperties, type ReactNode, type RefObject } from "react";
import { X } from "lucide-react";
import { FormDialog } from "../shared/FormDialog";

export function ExpandedComposer({ anchorRef, onClose, children }: {
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
  children: ReactNode;
}) {
  const titleId = useId();
  const [position, setPosition] = useState<CSSProperties>({});
  useLayoutEffect(() => {
    const anchor = anchorRef.current;
    if (!anchor) return;
    const update = () => {
      const rect = anchor.getBoundingClientRect();
      const width = Math.min(rect.width + 160, window.innerWidth - 24);
      const next = { width, left: Math.max(12, Math.min(rect.left - 136, window.innerWidth - width - 12)),
        bottom: Math.max(12, window.innerHeight - rect.bottom) };
      setPosition(previous => previous.width === next.width && previous.left === next.left && previous.bottom === next.bottom ? previous : next);
    };
    update();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    observer?.observe(anchor);
    window.addEventListener("resize", update);
    window.visualViewport?.addEventListener("resize", update);
    return () => { observer?.disconnect(); window.removeEventListener("resize", update);
      window.visualViewport?.removeEventListener("resize", update); };
  }, [anchorRef]);
  useEffect(() => {
    anchorRef.current?.ownerDocument.querySelector<HTMLTextAreaElement>('[role="dialog"] textarea')?.focus();
  }, [anchorRef]);
  return <FormDialog titleId={titleId} onCancel={onClose}
    className="ct-expanded-composer" style={position}
    onSubmit={(event) => event.preventDefault()}>
    <h2 id={titleId} className="sr-only">Message editor</h2>
    <button type="button" className="ct-expanded-composer__close" aria-label="Close expanded editor" onClick={onClose}>
      <X size={18} aria-hidden />
    </button>
    {children}
  </FormDialog>;
}

export function ComposerExpandHandle({ onExpand }: { onExpand: () => void }) {
  return <button type="button" className="ct-composer-expand" aria-label="Expand message editor" onClick={onExpand}>
    <svg width="36" height="8" viewBox="0 0 36 8" aria-hidden="true"><path d="M1 6 L18 2 L35 6" /></svg>
    <span>Show</span>
  </button>;
}
