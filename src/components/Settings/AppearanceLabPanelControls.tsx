import { Check, Contrast, LayoutGrid, Palette, RectangleHorizontal, Sparkles, Type, Undo2, X } from "lucide-react";
import { useId, useRef, type ReactNode, type RefObject } from "react";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import type { AppearanceLabCategory, AppearanceLabChange } from "./AppearanceLabChanges";

export const LAB_CATEGORIES = [
  { id: "presets", label: "Presets", icon: Sparkles },
  { id: "palette", label: "Palette", icon: Palette },
  { id: "wallpaper", label: "Wallpaper", icon: LayoutGrid },
  { id: "shape", label: "Shape", icon: RectangleHorizontal },
  { id: "text", label: "Text", icon: Type },
] as const;

export function AppearanceLabCategories({
  controlsId, category, changes, busy, comparing, menuOpen, machineSelector,
  onCategory, onMenuChange, onCompare, onBack,
}: {
  controlsId: string;
  category: AppearanceLabCategory;
  changes: AppearanceLabChange[];
  busy: boolean;
  comparing: boolean;
  menuOpen: boolean;
  machineSelector: ReactNode;
  onCategory(category: AppearanceLabCategory): void;
  onMenuChange(open: boolean): void;
  onCompare(value: boolean): void;
  onBack(): void;
}) {
  const menuId = useId();
  const menuTrigger = useRef<HTMLButtonElement>(null);
  return <div className="ct-appearance-lab ct-appearance-try__categories" role="toolbar" aria-label="Appearance">
    {LAB_CATEGORIES.map(({ id, label, icon: Icon }) => {
      const total = changes.filter(change => change.category === id).length;
      return <button type="button" key={id} aria-label={label} aria-pressed={category === id} aria-controls={controlsId}
        onClick={() => onCategory(id)} disabled={busy}>
        <Icon size={15} aria-hidden="true" /><span>{label}</span>
        {total > 0 && <span className="ct-appearance-try__badge" aria-hidden="true">{total}</span>}
      </button>;
    })}
    <button className="ct-appearance-try__compare" type="button" aria-label="Compare" title="Hold to compare with the applied look"
      aria-pressed={comparing} disabled={!changes.length || busy}
      onPointerDown={event => { event.currentTarget.setPointerCapture(event.pointerId); onCompare(true); }}
      onPointerUp={() => onCompare(false)} onPointerCancel={() => onCompare(false)} onLostPointerCapture={() => onCompare(false)}
      onKeyDown={event => { if (event.key === " " || event.key === "Enter") { event.preventDefault(); onCompare(true); } }}
      onKeyUp={event => { if (event.key === " " || event.key === "Enter") onCompare(false); }}
      onBlur={() => onCompare(false)}><Contrast size={15} aria-hidden="true" /><span>Compare</span></button>
    <button ref={menuTrigger} type="button" className="ct-appearance-try__menu" aria-label="More appearance options"
      aria-expanded={menuOpen} aria-controls={menuOpen ? menuId : undefined} onClick={() => onMenuChange(!menuOpen)}>⋯</button>
    <AnchoredPopover open={menuOpen} anchor={menuTrigger} placement="bottom-end" gap={6}
      className="ct-appearance-lab ct-appearance-try__popover" onClose={() => onMenuChange(false)}>
      <div id={menuId} className="ct-appearance-try__menu-content">{machineSelector}<button type="button" disabled={busy} onClick={onBack}>Back</button></div>
    </AnchoredPopover>
  </div>;
}

export function AppearanceLabActions({
  changes, changesTrigger, listOpen, busy, canApply, applyLabel, appliedStatus, undoLabel,
  onToggleList, onRevert, onDiscard, onApply, onUndo,
}: {
  changes: AppearanceLabChange[];
  changesTrigger: RefObject<HTMLButtonElement | null>;
  listOpen: boolean;
  busy: boolean;
  canApply: boolean;
  applyLabel: string;
  appliedStatus?: ReactNode;
  undoLabel?: string;
  onToggleList(): void;
  onRevert(change: AppearanceLabChange): void;
  onDiscard(): void;
  onApply(button: HTMLButtonElement): void;
  onUndo(): void;
}) {
  const count = changes.length === 1 ? "1 change" : `${changes.length} changes`;
  const changesId = useId();
  return <footer className="ct-appearance-lab ct-appearance-try__footer" role="region" aria-label="Changes">
    <div className="ct-appearance-try__actions">
      <div className="ct-appearance-changes__status" role="status" aria-live="polite" data-state={appliedStatus ? "applied" : undefined}>
        {appliedStatus
          ? <><Check size={15} aria-hidden="true" />{appliedStatus}</>
          : changes.length > 0 ? <button ref={changesTrigger} type="button" aria-expanded={listOpen} aria-controls={listOpen ? changesId : undefined}
            onClick={onToggleList}>{count}</button>
            : <span className="sr-only">{count}</span>}
      </div>
      {changes.length > 0 && <button type="button" className="ct-appearance-changes__discard" aria-label="Discard changes" disabled={busy} onClick={onDiscard}>Discard</button>}
      {changes.length > 0 && !appliedStatus && <button type="button" className="ct-appearance-primary ct-appearance-changes__apply" disabled={!canApply}
        onClick={event => onApply(event.currentTarget)}><Check size={15} aria-hidden="true" /><span>{applyLabel}</span></button>}
      {undoLabel && <button type="button" className="ct-appearance-changes__undo" aria-label={undoLabel} disabled={busy} onClick={onUndo}>
        <Undo2 size={15} aria-hidden="true" /><span>{undoLabel}</span>
      </button>}
    </div>
    {listOpen && <ul id={changesId} className="ct-appearance-changes__list" aria-label="Staged changes">
      {changes.map((change, index) => <li key={change.label}>
        <span>{change.label} {change.before} → {change.after}</span>
        <button type="button" autoFocus={index === 0} aria-label={`Revert ${change.label}`} disabled={busy} onClick={() => onRevert(change)}><X size={13} aria-hidden="true" /></button>
      </li>)}
    </ul>}
  </footer>;
}
