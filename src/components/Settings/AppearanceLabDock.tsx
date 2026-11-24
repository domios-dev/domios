import { Check, ChevronDown, ChevronUp, Contrast, LayoutGrid, Palette, RectangleHorizontal, Sparkles, Type, Undo2, X } from "lucide-react";
import { useId, useMemo, useRef, type ReactNode } from "react";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import type { AppearanceLabCategory, AppearanceLabChange } from "./AppearanceLabChanges";

export const LAB_CATEGORIES = [
  { id: "presets", label: "Presets", icon: Sparkles },
  { id: "palette", label: "Palette", icon: Palette },
  { id: "wallpaper", label: "Wallpaper", icon: LayoutGrid },
  { id: "shape", label: "Shape", icon: RectangleHorizontal },
  { id: "text", label: "Text", icon: Type },
] as const;

export type AppearanceLabPopover = "changes" | "menu";

interface Props {
  cardId: string;
  card: ReactNode;
  previewTag: ReactNode;
  category?: AppearanceLabCategory;
  changes: AppearanceLabChange[];
  popover?: AppearanceLabPopover;
  busy: boolean;
  canApply: boolean;
  comparing: boolean;
  applyLabel: string;
  appliedStatus?: ReactNode;
  undoLabel?: string;
  machineSelector: ReactNode;
  onCategory(category: AppearanceLabCategory): void;
  onPopoverChange(popover?: AppearanceLabPopover): void;
  onCompare(value: boolean): void;
  onRevert(change: AppearanceLabChange): void;
  onDiscard(): void;
  onApply(button: HTMLButtonElement): void;
  onUndo(): void;
  onBack(): void;
  onDismissApplied(): void;
}

export function AppearanceLabDock({
  cardId, card, previewTag, category, changes, popover, busy, canApply, comparing, applyLabel, appliedStatus, undoLabel, machineSelector,
  onCategory, onPopoverChange, onCompare, onRevert, onDiscard, onApply, onUndo, onBack, onDismissApplied,
}: Props) {
  const count = changes.length === 1 ? "1 change" : `${changes.length} changes`;
  const popoverId = useId();
  const changesId = `${popoverId}-changes`;
  const menuId = `${popoverId}-options`;
  const sheetRef = useRef<HTMLDivElement>(null);
  const dockRef = useRef<HTMLDivElement>(null);
  const changesTrigger = useRef<HTMLButtonElement>(null);
  const menuTrigger = useRef<HTMLButtonElement>(null);
  const positionAnchor = useMemo(() => ({
    get current() {
      const sheet = sheetRef.current;
      return sheet?.getClientRects().length ? sheet : dockRef.current;
    },
  }), []);
  const listOpen = popover === "changes";
  const menuOpen = popover === "menu";
  const dragStart = useRef<number | undefined>(undefined);
  const dragged = useRef(false);
  return <div ref={sheetRef} className="ct-appearance-try__sheet">
    {previewTag}
    {card}
    <div ref={dockRef} className="ct-appearance-try__dock" role="toolbar" aria-label="Appearance">
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
            : changes.length > 0 ? <button ref={changesTrigger} type="button" aria-expanded={listOpen} aria-controls={listOpen ? changesId : undefined}
              onClick={() => onPopoverChange(listOpen ? undefined : "changes")}>{count}</button>
              : <span className="sr-only">{count}</span>}
        </div>
        <AnchoredPopover open={listOpen} anchor={changesTrigger} positionAnchor={positionAnchor} placement="top-end" gap={6}
          className="ct-appearance-lab ct-appearance-try__popover" onClose={onPopoverChange}>
          <ul id={changesId} className="ct-appearance-changes__list" aria-label="Staged changes">
            {changes.map((change, index) => <li key={change.label}>
              <span>{change.label} {change.before} → {change.after}</span>
              <button type="button" autoFocus={index === 0} aria-label={`Revert ${change.label}`} disabled={busy} onClick={() => onRevert(change)}><X size={13} aria-hidden="true" /></button>
            </li>)}
          </ul>
        </AnchoredPopover>
        {changes.length > 0 && <button type="button" className="ct-appearance-changes__discard" aria-label="Discard changes" disabled={busy} onClick={onDiscard}>Discard</button>}
        {changes.length > 0 && !appliedStatus && <button type="button" className="ct-appearance-primary ct-appearance-changes__apply" disabled={!canApply}
          onClick={event => onApply(event.currentTarget)}><Check size={15} aria-hidden="true" /><span>{applyLabel}</span></button>}
        {undoLabel && <button type="button" className="ct-appearance-changes__undo" aria-label={undoLabel} disabled={busy} onClick={onUndo}>
          <Undo2 size={15} aria-hidden="true" /><span className="ct-appearance-changes__wide">{undoLabel}</span><span className="ct-appearance-changes__short">Undo</span>
        </button>}
        {appliedStatus && <button type="button" aria-label="Dismiss applied appearance" disabled={busy} onClick={onDismissApplied}>
          <X size={15} aria-hidden="true" />
        </button>}
      </div>
      <button className="ct-appearance-try__compare" type="button" aria-label="Compare" title="Hold to compare with the applied look"
        aria-pressed={comparing} disabled={!changes.length || busy}
        onPointerDown={event => { event.currentTarget.setPointerCapture(event.pointerId); onCompare(true); }}
        onPointerUp={() => onCompare(false)} onPointerCancel={() => onCompare(false)} onLostPointerCapture={() => onCompare(false)}
        onKeyDown={event => { if (event.key === " " || event.key === "Enter") { event.preventDefault(); onCompare(true); } }}
        onKeyUp={event => { if (event.key === " " || event.key === "Enter") onCompare(false); }}
        onBlur={() => onCompare(false)}><Contrast size={15} aria-hidden="true" /><span>Compare</span></button>
      <button ref={menuTrigger} type="button" className="ct-appearance-try__menu" aria-label="More appearance options"
        aria-expanded={menuOpen} aria-controls={menuOpen ? menuId : undefined} onClick={() => onPopoverChange(menuOpen ? undefined : "menu")}>⋯</button>
      <AnchoredPopover open={menuOpen} anchor={menuTrigger} positionAnchor={positionAnchor} placement="top-end" gap={6}
        className="ct-appearance-lab ct-appearance-try__popover" onClose={onPopoverChange}>
        <div id={menuId} className="ct-appearance-try__menu-content">{machineSelector}<button type="button" disabled={busy} onClick={onBack}>Back</button></div>
      </AnchoredPopover>
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
    </div>
  </div>;
}
