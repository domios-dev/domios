import { useEffect, type ReactNode, type RefObject, type HTMLAttributes } from "react";
import { X } from "lucide-react";
import { BackPill } from "../../shared/BackPill";
import type { PresenceState } from "../usePresence";

export function SidePanelFrame({ panelRef, state, label, title, actions, children, onClose, contentKey, replaced, backLabel, closeLabel, ...attributes }: {
  panelRef: RefObject<HTMLElement | null>; state: PresenceState; label: string;
  title: ReactNode; actions?: ReactNode; children: ReactNode; onClose: () => void;
  contentKey?: string; replaced?: boolean; backLabel?: string; closeLabel?: string;
} & Omit<HTMLAttributes<HTMLElement>, "title">) {
  useEffect(() => { panelRef.current?.focus(); }, [panelRef, contentKey]);
  return <aside {...attributes} ref={panelRef} data-plan-side-panel="" role="dialog" aria-modal="false" aria-label={label} tabIndex={-1} data-state={state} inert={state === "closing" || undefined} className="plan-side-panel" onKeyDown={(event) => {
    if ((event.code || event.key) !== "Escape" || event.defaultPrevented) return;
    event.preventDefault(); event.stopPropagation(); onClose();
  }}>
    <header className="plan-side-panel__header">{title}{actions}
      {backLabel ? <BackPill label={backLabel} testId="plan-side-panel-back" onBack={onClose} /> :
        <button type="button" data-testid="plan-side-panel-close" aria-label={closeLabel ?? `Close ${label}`} title="Close (Esc)" className="plan-side-panel__action" onClick={onClose}><X size={15} aria-hidden /></button>}
    </header>
    <div key={contentKey} data-testid="plan-side-panel-body" data-replaced={replaced ? "" : undefined} className="plan-side-panel__body plan-detail-selectable">{children}</div>
  </aside>;
}
