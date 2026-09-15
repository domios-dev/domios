import { useEffect, useId, useRef } from "react";
import { X } from "lucide-react";
import type { AnalyticsModelKey } from "@codeterm/plugin-sdk";
import { Segmented } from "../../../../../src/components/shared/Segmented";
import { formatCount } from "../model/format";
import {
  modelOptions,
  onlyUnrecorded,
  providerOptions,
  recordOptions,
  type Catalog,
  type FilterOption,
} from "../model/options";
import { activeFilterCount, NO_FILTERS, sameModel, toggled, type FilterGroupId, type FilterState } from "../model/query";
import { RANGE_OPTIONS, type RangePreset } from "../model/range";

export type OpenFilter = FilterGroupId | "sheet" | null;

export const FILTER_GROUPS: Array<{ id: FilterGroupId; label: string }> = [
  { id: "providers", label: "Provider" },
  { id: "models", label: "Model" },
  { id: "workspaces", label: "Workspace" },
  { id: "teams", label: "Team" },
  { id: "plans", label: "Plan" },
];

function optionsFor(group: FilterGroupId, catalog: Catalog): FilterOption<unknown>[] {
  switch (group) {
    case "providers":
      return providerOptions(catalog);
    case "models":
      return modelOptions(catalog);
    case "workspaces":
      return recordOptions(catalog.workspace);
    case "teams":
      return recordOptions(catalog.team);
    case "plans":
      return recordOptions(catalog.plan);
  }
}

function chosen(group: FilterGroupId, filters: FilterState, value: unknown): boolean {
  if (group === "models") return filters.models.some((model) => sameModel(model, value as AnalyticsModelKey));
  return (filters[group] as unknown[]).includes(value);
}

function toggle(group: FilterGroupId, filters: FilterState, value: unknown): FilterState {
  if (group === "models") return { ...filters, models: toggled(filters.models, value as AnalyticsModelKey, sameModel) };
  return { ...filters, [group]: toggled(filters[group] as unknown[], value) };
}

function FilterOptions({
  group,
  label,
  catalog,
  filters,
  onFilters,
}: {
  group: FilterGroupId;
  label: string;
  catalog: Catalog;
  filters: FilterState;
  onFilters: (next: FilterState) => void;
}) {
  const options = optionsFor(group, catalog);
  if (group === "workspaces" && onlyUnrecorded(options as FilterOption<string | null>[])) {
    return <p className="mc-board-note">No turn in this range recorded its workspace, so there is nothing to filter by yet.</p>;
  }
  if (options.length === 0) return <p className="mc-board-note">Nothing in this range to filter by.</p>;
  return (
    <div role="group" aria-label={label} className="mc-board-chips analytics-options">
      {options.map((option) => (
        <button
          key={option.id}
          type="button"
          data-testid={`analytics-option-${group}`}
          className="mc-board-chip"
          data-unnamed={option.unnamed ? "true" : undefined}
          aria-pressed={chosen(group, filters, option.value)}
          onClick={() => onFilters(toggle(group, filters, option.value))}
        >
          {option.label}
          {option.detail && <span className="analytics-option-detail"> · {option.detail}</span>}
        </button>
      ))}
    </div>
  );
}

function FilterSheet({
  catalog,
  filters,
  onFilters,
  onClose,
  turnsInView,
}: {
  catalog: Catalog;
  filters: FilterState;
  onFilters: (next: FilterState) => void;
  onClose: () => void;
  turnsInView: number | null;
}) {
  const titleId = useId();
  const sheet = useRef<HTMLDivElement>(null);
  useEffect(() => sheet.current?.querySelector<HTMLButtonElement>("button")?.focus(), []);
  return (
    <div className="analytics-sheet-backdrop" onClick={onClose}>
      <div
        ref={sheet}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        data-testid="analytics-filter-sheet"
        className="analytics-sheet"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="analytics-sheet-head">
          <h2 id={titleId}>Filters</h2>
          <button type="button" className="mc-board-more" onClick={() => onFilters(NO_FILTERS)}>
            Clear all
          </button>
          <button type="button" className="mc-board-chip analytics-icon-btn" aria-label="Close filters" onClick={onClose}>
            <X size={16} aria-hidden />
          </button>
        </div>
        <div className="analytics-sheet-body">
          {FILTER_GROUPS.map((group) => (
            <section key={group.id} className="analytics-sheet-group">
              <h3 className="mc-board-sub-label">{group.label}</h3>
              <FilterOptions group={group.id} label={group.label} catalog={catalog} filters={filters} onFilters={onFilters} />
            </section>
          ))}
        </div>
        <div className="analytics-sheet-foot">
          <p className="mc-board-caption">Choices inside a group widen the view; choices across groups narrow it.</p>
          <button type="button" className="mc-board-chip analytics-primary" onClick={onClose}>
            {turnsInView === null ? "Show results" : `Show ${formatCount(turnsInView)} turns`}
          </button>
        </div>
      </div>
    </div>
  );
}

export function FilterControls({
  preset,
  onPreset,
  filters,
  onFilters,
  catalog,
  rangeText,
  narrow,
  open,
  onOpen,
  turnsInView,
}: {
  preset: RangePreset;
  onPreset: (preset: RangePreset) => void;
  filters: FilterState;
  onFilters: (next: FilterState) => void;
  catalog: Catalog;
  rangeText: string;
  narrow: boolean;
  open: OpenFilter;
  onOpen: (open: OpenFilter) => void;
  turnsInView: number | null;
}) {
  const panelBase = useId();
  const bar = useRef<HTMLDivElement>(null);
  const count = activeFilterCount(filters);

  useEffect(() => {
    if (open === null || open === "sheet") return;
    bar.current?.querySelector<HTMLButtonElement>(`[data-panel="${open}"] button`)?.focus();
    const onPointer = (event: PointerEvent) => {
      if (!(event.target as Element | null)?.closest?.(".analytics-filter")) onOpen(null);
    };
    document.addEventListener("pointerdown", onPointer);
    return () => document.removeEventListener("pointerdown", onPointer);
  }, [open, onOpen]);

  const range = (
    <Segmented
      value={preset}
      options={RANGE_OPTIONS}
      onChange={onPreset}
      aria-label="Range"
      data-testid="analytics-range"
      height={narrow ? 36 : 26}
      className="analytics-range"
    />
  );

  if (narrow) {
    return (
      <div className="analytics-filterbar">
        {range}
        <div className="analytics-filter-row">
          <button
            type="button"
            data-testid="analytics-filters-open"
            className="mc-board-chip"
            aria-haspopup="dialog"
            aria-expanded={open === "sheet"}
            onClick={() => onOpen("sheet")}
          >
            Filters · {count > 0 ? `${count} set` : "none set"}
          </button>
          <span className="mc-board-caption">{rangeText}</span>
        </div>
        {open === "sheet" && (
          <FilterSheet catalog={catalog} filters={filters} onFilters={onFilters} onClose={() => onOpen(null)} turnsInView={turnsInView} />
        )}
      </div>
    );
  }

  return (
    <div ref={bar} className="analytics-filterbar">
      {range}
      {FILTER_GROUPS.map((group) => {
        const selected = filters[group.id].length;
        const panelId = `${panelBase}-${group.id}`;
        return (
          <div key={group.id} className="analytics-filter">
            <button
              type="button"
              data-testid={`analytics-filter-${group.id}`}
              className="mc-board-chip"
              aria-haspopup="dialog"
              aria-expanded={open === group.id}
              aria-controls={open === group.id ? panelId : undefined}
              onClick={() => onOpen(open === group.id ? null : group.id)}
            >
              {group.label} <b>{selected > 0 ? selected : "All"}</b>
            </button>
            {open === group.id && (
              <div id={panelId} role="dialog" aria-label={`${group.label} filter`} data-panel={group.id} className="analytics-popover">
                <FilterOptions group={group.id} label={group.label} catalog={catalog} filters={filters} onFilters={onFilters} />
              </div>
            )}
          </div>
        );
      })}
      {count > 0 && (
        <button type="button" data-testid="analytics-clear-filters" className="mc-board-more" onClick={() => onFilters(NO_FILTERS)}>
          Clear filters
        </button>
      )}
      <span className="mc-board-caption analytics-range-caption">{rangeText}</span>
    </div>
  );
}
