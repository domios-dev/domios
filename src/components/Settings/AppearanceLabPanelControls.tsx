import { Check, ChevronDown, Contrast, LayoutGrid, Palette, RectangleHorizontal, Sparkles, TriangleAlert, Type, Undo2, X } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import type { ContrastCheck } from "../../lib/appearance";
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
  const selected = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => {
    const row = selected.current?.parentElement;
    if (row && row.scrollWidth > row.clientWidth) selected.current?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [category]);
  const compareButton = (placement: "row" | "overflow") =>
    <button className={`ct-appearance-try__compare--${placement}`} type="button" aria-label="Compare" title="Hold to compare with the applied look"
      aria-pressed={comparing} disabled={!changes.length || busy}
      onPointerDown={event => { event.currentTarget.setPointerCapture(event.pointerId); onCompare(true); }}
      onPointerUp={() => onCompare(false)} onPointerCancel={() => onCompare(false)} onLostPointerCapture={() => onCompare(false)}
      onKeyDown={event => { if (event.key === " " || event.key === "Enter") { event.preventDefault(); onCompare(true); } }}
      onKeyUp={event => { if (event.key === " " || event.key === "Enter") onCompare(false); }}
      onBlur={() => onCompare(false)}><Contrast size={15} aria-hidden="true" /><span>Compare</span></button>;
  return <div className="ct-appearance-lab ct-appearance-try__categories" role="toolbar" aria-label="Appearance">
    <div className="ct-appearance-try__category-list">
      {LAB_CATEGORIES.map(({ id, label, icon: Icon }) => {
        const total = changes.filter(change => change.category === id).length;
        return <button type="button" key={id} ref={category === id ? selected : undefined}
          aria-label={label} aria-pressed={category === id} aria-controls={controlsId}
          onClick={() => onCategory(id)} disabled={busy}>
          <Icon size={15} aria-hidden="true" /><span>{label}</span>
          {total > 0 && <span className="ct-appearance-try__badge" aria-hidden="true">{total}</span>}
        </button>;
      })}
      {compareButton("row")}
    </div>
    <button ref={menuTrigger} type="button" className="ct-appearance-try__menu" aria-label="More appearance options"
      aria-expanded={menuOpen} aria-controls={menuOpen ? menuId : undefined} onClick={() => onMenuChange(!menuOpen)}>⋯</button>
    <AnchoredPopover open={menuOpen} anchor={menuTrigger} placement="bottom-end" gap={6}
      className="ct-appearance-lab ct-appearance-try__popover" onClose={() => onMenuChange(false)}>
      <div id={menuId} className="ct-appearance-try__menu-content">
        {machineSelector}
        {compareButton("overflow")}
        <button type="button" disabled={busy} onClick={onBack}>Back</button>
      </div>
    </AnchoredPopover>
  </div>;
}

export interface ContrastIssue {
  label: string;
  ratio: number;
  floor: number;
  surfaces: string[];
  fix: ContrastCheck["fix"];
}

// One entry per failing element, worst pair first, so a single bad colour reads as one issue.
export function contrastIssues(failures: ContrastCheck[]): ContrastIssue[] {
  const groups = new Map<string, ContrastCheck[]>();
  for (const failure of failures) groups.set(failure.name, [...groups.get(failure.name) ?? [], failure]);
  return [...groups.entries()].map(([name, checks]) => {
    const worst = checks.reduce((low, check) => check.ratio < low.ratio ? check : low);
    const prefix = `${name} on `;
    return {
      label: checks.length === 1 ? worst.pair : name,
      ratio: worst.ratio,
      floor: worst.floor,
      surfaces: checks.length === 1 ? [] : checks.map(check => check.pair.startsWith(prefix) ? check.pair.slice(prefix.length) : check.pair),
      fix: checks.find(check => check.fix)?.fix ?? null,
    };
  });
}

function ContrastSummary({ issues, id, busy, onFix, onFixAll }: {
  issues: ContrastIssue[];
  id: string;
  busy: boolean;
  onFix(fix: NonNullable<ContrastCheck["fix"]>): void;
  onFixAll(): void;
}) {
  const [open, setOpen] = useState(false);
  const listId = useId();
  const count = issues.length === 1 ? "1 contrast issue" : `${issues.length} contrast issues`;
  useEffect(() => {
    if (issues.length === 0) setOpen(false);
  }, [issues.length]);
  if (issues.length === 0) return null;
  return <>
    <button type="button" id={id} className="ct-appearance-try__contrast" aria-expanded={open} aria-controls={open ? listId : undefined}
      onClick={() => setOpen(value => !value)}>
      <TriangleAlert size={14} aria-hidden="true" /><span>{count}</span>
      <ChevronDown size={13} aria-hidden="true" data-open={open || undefined} />
    </button>
    {open && <div id={listId} className="ct-appearance-contrast" role="group" aria-label="Contrast problems">
      <ul>
        {issues.map(issue => <li key={issue.label}>
          <span>
            <b>{issue.label}</b> {issue.ratio.toFixed(2)}:1 · needs {issue.floor}:1
            {issue.surfaces.length > 0 && <small>on {issue.surfaces.join(", ")}</small>}
          </span>
          {issue.fix && <button type="button" disabled={busy} aria-label={`Fix ${issue.label}`}
            onClick={() => issue.fix && onFix(issue.fix)}>Fix</button>}
        </li>)}
      </ul>
      {issues.filter(issue => issue.fix).length > 1 && <button type="button" className="ct-appearance-contrast__all" disabled={busy}
        onClick={onFixAll}>Fix all</button>}
    </div>}
  </>;
}

export function AppearanceLabActions({
  changes, changesTrigger, listOpen, focusChanges, busy, canApply, applyLabel, appliedStatus, undoLabel,
  contrast, onFixContrast, onFixAllContrast, onToggleList, onRevert, onDiscard, onApply, onUndo,
}: {
  changes: AppearanceLabChange[];
  changesTrigger: RefObject<HTMLButtonElement | null>;
  listOpen: boolean;
  focusChanges: boolean;
  busy: boolean;
  canApply: boolean;
  applyLabel: string;
  appliedStatus?: ReactNode;
  undoLabel?: string;
  contrast: ContrastIssue[];
  onFixContrast(fix: NonNullable<ContrastCheck["fix"]>): void;
  onFixAllContrast(): void;
  onToggleList(): void;
  onRevert(change: AppearanceLabChange): void;
  onDiscard(): void;
  onApply(button: HTMLButtonElement): void;
  onUndo(): void;
}) {
  const count = changes.length === 1 ? "1 change" : `${changes.length} changes`;
  const changesId = useId();
  const contrastId = useId();
  return <footer className="ct-appearance-lab ct-appearance-try__footer" role="region" aria-label="Changes">
    <ContrastSummary issues={contrast} id={contrastId} busy={busy} onFix={onFixContrast} onFixAll={onFixAllContrast} />
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
        aria-describedby={contrast.length > 0 ? contrastId : undefined}
        onClick={event => onApply(event.currentTarget)}><Check size={15} aria-hidden="true" /><span>{applyLabel}</span></button>}
      {undoLabel && <button type="button" className="ct-appearance-changes__undo" aria-label={undoLabel} disabled={busy} onClick={onUndo}>
        <Undo2 size={15} aria-hidden="true" /><span>{undoLabel}</span>
      </button>}
    </div>
    {listOpen && <ul id={changesId} className="ct-appearance-changes__list" aria-label="Staged changes">
      {changes.map((change, index) => <li key={change.label}>
        <span>{change.label} {change.before} → {change.after}</span>
        <button type="button" autoFocus={focusChanges && index === 0} aria-label={`Revert ${change.label}`} disabled={busy} onClick={() => onRevert(change)}><X size={13} aria-hidden="true" /></button>
      </li>)}
    </ul>}
  </footer>;
}
