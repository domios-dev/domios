import { useEffect, useId, useLayoutEffect, useState, type CSSProperties, type ReactNode, type RefObject } from "react";
import { X } from "lucide-react";
import { FormDialog } from "../shared/FormDialog";

function measureExpandedPosition(anchor: HTMLElement): CSSProperties {
  const rect = anchor.getBoundingClientRect();
  const vvHeight = window.visualViewport?.height ?? 0;
  const viewportHeight = vvHeight > 0 ? vvHeight : window.innerHeight;
  const width = Math.min(rect.width + 160, window.innerWidth - 24);
  const bottom = Math.max(12, Math.min(Math.max(12, viewportHeight - 220), window.innerHeight - rect.bottom));
  const maxHeight = Math.max(200, Math.floor(viewportHeight - bottom - 16));
  return {
    width,
    left: Math.max(12, Math.min(rect.left - 136, window.innerWidth - width - 12)),
    bottom,
    maxHeight,
  };
}

function samePosition(a: CSSProperties, b: CSSProperties): boolean {
  return a.width === b.width && a.left === b.left && a.bottom === b.bottom && a.maxHeight === b.maxHeight;
}

export function ExpandedComposer({ anchorRef, onClose, headerStatus, children }: {
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
  headerStatus?: ReactNode;
  children: ReactNode;
}) {
  const titleId = useId();
  const [position, setPosition] = useState<CSSProperties>({ bottom: 12, maxHeight: "calc(100dvh - 28px)" });
  useLayoutEffect(() => {
    const update = () => {
      const anchor = anchorRef.current;
      if (!anchor) return;
      const next = measureExpandedPosition(anchor);
      setPosition(previous => samePosition(previous, next) ? previous : next);
    };
    update();
    const anchor = anchorRef.current;
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    if (anchor) observer?.observe(anchor);
    window.addEventListener("resize", update);
    window.visualViewport?.addEventListener("resize", update);
    return () => { observer?.disconnect(); window.removeEventListener("resize", update);
      window.visualViewport?.removeEventListener("resize", update); };
  }, [anchorRef]);
  useEffect(() => {
    const anchor = anchorRef.current;
    if (anchor) {
      const next = measureExpandedPosition(anchor);
      setPosition(previous => samePosition(previous, next) ? previous : next);
    }
    anchor?.ownerDocument.querySelector<HTMLTextAreaElement>('[role="dialog"] textarea')?.focus();
  }, [anchorRef]);
  return <FormDialog titleId={titleId} onCancel={onClose}
    className="ct-expanded-composer" style={position}
    onSubmit={(event) => event.preventDefault()}>
    <h2 id={titleId} className="sr-only">Message editor</h2>
    <div className="ct-expanded-composer__header">
      {headerStatus ? <div className="ct-expanded-composer__status">{headerStatus}</div> : null}
      <button type="button" className="ct-expanded-composer__close" aria-label="Close expanded editor" onClick={onClose}>
        <X size={18} aria-hidden />
      </button>
    </div>
    {children}
  </FormDialog>;
}

export function ComposerExpandHandle({ onExpand }: { onExpand: () => void }) {
  return <button type="button" className="ct-composer-expand" aria-label="Expand message editor" onClick={onExpand}>
    <svg width="36" height="8" viewBox="0 0 36 8" aria-hidden="true"><path d="M1 6 L18 2 L35 6" /></svg>
    <span>Show</span>
  </button>;
}
