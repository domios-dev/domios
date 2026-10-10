import { useId, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { ChevronRight, X } from "lucide-react";
import { focusLost } from "./focus";
import type { PresenceState } from "../Canvas/usePresence";
import { AUTOMATION_LABEL } from "./styles";

export function Facts({ children }: { children: ReactNode }) {
  return <dl className="ct-detail__lane break-words">{children}</dl>;
}

export function Disclosure({ title, children }: { title: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const region = useId();
  return <section className="min-w-0">
    <button type="button" className={`${AUTOMATION_LABEL} -mx-1 inline-flex items-center gap-1 rounded px-1 py-0.5 hover:text-[var(--ct-fg)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--ct-accent)]`} aria-expanded={open} aria-controls={region} onClick={() => setOpen(!open)}>
      <ChevronRight size={12} className={`transition-transform duration-[var(--ct-duration-liquid-hover)] ease-[var(--ct-ease-liquid-settle)] motion-reduce:transition-none ${open ? "rotate-90" : ""}`} aria-hidden="true" />{title}
    </button>
    <div id={region} hidden={!open} className="mt-2">{children}</div>
  </section>;
}

interface FrameProps {
  label: string; title: ReactNode; onClose: () => void; children: ReactNode; focusWhenLost?: boolean;
  panelRef?: RefObject<HTMLElement | null>; presence?: PresenceState;
}

/** The app's right side-panel surface: a non-modal region over the graph with its own scroll, Escape and focus-on-open. */
export function DetailsFrame({ label, title, onClose, children, focusWhenLost = false, panelRef, presence }: FrameProps) {
  const own = useRef<HTMLElement>(null);
  const panel = panelRef ?? own;
  useLayoutEffect(() => {
    if (!focusWhenLost || focusLost()) panel.current?.focus();
  }, [label, focusWhenLost, panel]);
  return <aside ref={panel} tabIndex={-1} role="region" aria-label={label} data-state={presence} inert={presence === "closing" || undefined} className="plan-side-panel text-[length:var(--ct-font-size-sm)] text-[var(--ct-fg)] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--ct-accent)]" onKeyDown={(event) => {
    if (event.code !== "Escape") return;
    event.stopPropagation();
    onClose();
  }}>
    <header className="plan-side-panel__header items-start">
      <div className="min-w-0 flex-1">{title}</div>
      <button type="button" className="plan-side-panel__action" aria-label="Close details" title="Close details (Esc)" onClick={onClose}><X size={15} aria-hidden="true" /></button>
    </header>
    <div className="plan-side-panel__body"><div className="ct-detail">{children}</div></div>
  </aside>;
}
