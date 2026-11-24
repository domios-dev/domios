import { Check, ChevronDown, ChevronUp, Contrast, LayoutGrid, Palette, RectangleHorizontal, Sparkles, Type, Undo2, X } from "lucide-react";
import { useRef, type ReactNode } from "react";
import type { AppearanceLabCategory, AppearanceLabChange } from "./AppearanceLabChanges";

export const LAB_CATEGORIES = [
  { id: "presets", label: "Presets", icon: Sparkles },
  { id: "palette", label: "Palette", icon: Palette },
  { id: "wallpaper", label: "Wallpaper", icon: LayoutGrid },
  { id: "shape", label: "Shape", icon: RectangleHorizontal },
  { id: "text", label: "Text", icon: Type },
] as const;

interface Props {
  cardId: string;
  category?: AppearanceLabCategory;
  changes: AppearanceLabChange[];
  listOpen: boolean;
  busy: boolean;
  canApply: boolean;
  comparing: boolean;
  applyLabel: string;
  appliedStatus?: ReactNode;
  undoLabel?: string;
  machineSelector: ReactNode;
  onCategory(category: AppearanceLabCategory): void;
  onToggleList(): void;
  onCompare(value: boolean): void;
  onRevert(change: AppearanceLabChange): void;
  onDiscard(): void;
  onApply(button: HTMLButtonElement): void;
  onUndo(): void;
  onBack(): void;
}

export function AppearanceLabDock({
  cardId, category, changes, listOpen, busy, canApply, comparing, applyLabel, appliedStatus, undoLabel, machineSelector,
  onCategory, onToggleList, onCompare, onRevert, onDiscard, onApply, onUndo, onBack,
}: Props) {
  const count = changes.length === 1 ? "1 change" : `${changes.length} changes`;
  const dragStart = useRef<number | undefined>(undefined);
  const dragged = useRef(false);
  return <div className="ct-appearance-try__dock" role="toolbar" aria-label="Appearance">
    <div className="ct-appearance-try__categories">
      {LAB_CATEGORIES.map(({ id, label, icon: Icon }) => {
        const total = changes.filter(change => change.category === id).length;
        return <button type="button" key={id} aria-label={label} aria-pressed={category === id} aria-controls={cardId}
          data-lab-category={id} onClick={() => onCategory(id)} disabled={Boolean(appliedStatus)}>
          <Icon size={15} aria-hidden="true" /><span className="ct-appearance-try__category-label">{label}</span>
          {total > 0 && <span className="ct-appearance-try__badge" aria-hidden="true">{total}</span>}
        </button>;
      })}
    </div>
    <div className="ct-appearance-try__actions" role="region" aria-label="Changes">
      <div className="ct-appearance-changes__status" role="status" aria-live="polite" data-state={appliedStatus ? "applied" : undefined}>
        {appliedStatus
          ? <><Check size={15} aria-hidden="true" />{appliedStatus}</>
          : <button type="button" aria-expanded={listOpen} disabled={!changes.length} onClick={onToggleList}>{count}</button>}
      </div>
      {listOpen && changes.length > 0 && <ul className="ct-appearance-changes__list" aria-label="Staged changes">
        {changes.map(change => <li key={change.label}>
          <span>{change.label} {change.before} → {change.after}</span>
          <button type="button" aria-label={`Revert ${change.label}`} disabled={busy} onClick={() => onRevert(change)}><X size={13} aria-hidden="true" /></button>
        </li>)}
      </ul>}
      {changes.length > 0 && <button type="button" className="ct-appearance-changes__discard" aria-label="Discard changes" disabled={busy} onClick={onDiscard}>Discard</button>}
      {!appliedStatus && <button type="button" className="ct-appearance-primary ct-appearance-changes__apply" disabled={!canApply}
        onClick={event => onApply(event.currentTarget)}><Check size={15} aria-hidden="true" />{applyLabel}</button>}
      {undoLabel && <button type="button" className="ct-appearance-changes__undo" aria-label={undoLabel} disabled={busy} onClick={onUndo}>
        <Undo2 size={15} aria-hidden="true" /><span className="ct-appearance-changes__wide">{undoLabel}</span><span className="ct-appearance-changes__short">Undo</span>
      </button>}
    </div>
    <button className="ct-appearance-try__compare" type="button" aria-label="Compare" title="Hold to compare with the applied look"
      aria-pressed={comparing} disabled={!changes.length || busy}
      onPointerDown={event => { event.currentTarget.setPointerCapture(event.pointerId); onCompare(true); }}
      onPointerUp={() => onCompare(false)} onPointerCancel={() => onCompare(false)} onLostPointerCapture={() => onCompare(false)}
      onKeyDown={event => { if (event.key === " " || event.key === "Enter") { event.preventDefault(); onCompare(true); } }}
      onKeyUp={event => { if (event.key === " " || event.key === "Enter") onCompare(false); }}
      onBlur={() => onCompare(false)}><Contrast size={15} aria-hidden="true" /><span>Compare</span></button>
    <details className="ct-appearance-try__menu">
      <summary aria-label="Appearance options">⋯</summary>
      <div>{machineSelector}<button type="button" disabled={busy} onClick={onBack}>Back</button></div>
    </details>
    <button type="button" className="ct-appearance-try__sheet-toggle" aria-label={category ? "Collapse appearance card" : "Expand appearance card"}
      onPointerDown={event => {
        dragged.current = false;
        if (!category) return;
        dragStart.current = event.clientY;
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerUp={event => {
        const start = dragStart.current;
        dragStart.current = undefined;
        if (category && start !== undefined && event.clientY - start > 35) {
          dragged.current = true;
          onCategory(category);
        }
      }}
      onPointerCancel={() => { dragStart.current = undefined; }}
      onClick={event => { if (!dragged.current || event.detail === 0) onCategory(category ?? "presets"); }}>
      {category ? <ChevronDown size={15} aria-hidden="true" /> : <><ChevronUp size={15} aria-hidden="true" /> Lab</>}
    </button>
  </div>;
}
