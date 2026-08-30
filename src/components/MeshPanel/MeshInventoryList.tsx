import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Check, ChevronDown, ChevronUp, RefreshCw, Search, SlidersHorizontal } from "lucide-react";
import { UI_BUTTON, UI_INPUT, UI_PANEL, UI_TEXT } from "../shared/design";
import { BusySpinner } from "../shared/BusyState";
import type { Service } from "./types";
import {
  buildInventoryModel,
  DEFAULT_INVENTORY_FILTERS,
  DEFAULT_INVENTORY_SORT,
  type InventoryFilters,
  type InventoryModel,
  type InventoryPeerIssue,
  type InventoryRow,
  type InventorySnapshot,
  type InventorySort,
  type InventorySortKey,
} from "./meshInventoryModel";

interface Props {
  resources: Service[];
  thisMachineName: string;
  peerIssues?: InventoryPeerIssue[];
  loading?: boolean;
  error?: string | null;
  selectedResourceKey?: string | null;
  onSelectResource: (resource: Service, resourceKey: string) => void;
  onRefresh?: () => void;
  headerActions?: ReactNode;
}

const STATE_OPTIONS = ["healthy", "degraded", "down", "unknown", "disabled", "running", "failed", "idle"] as const;

export function MeshInventoryList({
  resources,
  thisMachineName,
  peerIssues = [],
  loading = false,
  error = null,
  selectedResourceKey = null,
  onSelectResource,
  onRefresh,
  headerActions,
}: Props) {
  const [filters, setFilters] = useState<InventoryFilters>(DEFAULT_INVENTORY_FILTERS);
  const [sort, setSort] = useState<InventorySort>(DEFAULT_INVENTORY_SORT);
  const [announcement, setAnnouncement] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const focusedResourceKeyRef = useRef<string | null>(null);
  const announcementKeyRef = useRef<string | null>(null);
  const snapshot: InventorySnapshot = useMemo(
    () => ({ resources, peerIssues, loading, error }),
    [resources, peerIssues, loading, error],
  );
  const model = useMemo(
    () => buildInventoryModel(snapshot, filters, sort, thisMachineName),
    [snapshot, filters, sort, thisMachineName],
  );

  useEffect(() => {
    const handleSlash = (event: KeyboardEvent) => {
      const target = event.target;
      const inEditable = target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement || (target instanceof HTMLElement && target.isContentEditable);
      if (event.code !== "Slash" || inEditable) return;
      event.preventDefault();
      searchRef.current?.focus();
    };
    window.addEventListener("keydown", handleSlash);
    return () => window.removeEventListener("keydown", handleSlash);
  }, []);

  useEffect(() => {
    const trackedKeys = [focusedResourceKeyRef.current, selectedResourceKey].filter((key): key is string => Boolean(key));
    const trackedKey = trackedKeys.find((key) => !model.rows.some((row) => row.resourceKey === key)) ?? trackedKeys[0];
    if (!trackedKey) return;
    const selected = model.allRows.find((row) => row.resourceKey === trackedKey);
    const visible = model.rows.some((row) => row.resourceKey === trackedKey);
    if (visible) {
      announcementKeyRef.current = null;
      return;
    }
    const reason = selected
      ? "It is hidden by the current filters."
      : "It is no longer in the refreshed inventory.";
    const key = `${trackedKey}:${reason}:${model.total}:${model.visible}`;
    if (announcementKeyRef.current === key) return;
    announcementKeyRef.current = key;
    setAnnouncement(`Selected resource is no longer visible. ${reason}`);
    const active = document.activeElement as HTMLElement | null;
    if (active?.dataset.resourceKey === trackedKey) searchRef.current?.focus();
  }, [model, selectedResourceKey]);

  const updateFilter = <K extends keyof InventoryFilters>(key: K, value: InventoryFilters[K]) => {
    setFilters((current) => ({ ...current, [key]: value }));
  };

  const clearFilters = () => setFilters(DEFAULT_INVENTORY_FILTERS);

  const toggleSort = (key: InventorySortKey) => {
    setSort((current) => ({
      key,
      direction: current.key === key && current.direction === "asc" ? "desc" : "asc",
    }));
  };

  const moveFocus = (row: InventoryRow, direction: number) => {
    const buttons = Array.from(listRef.current?.querySelectorAll<HTMLButtonElement>("button[data-resource-key]") ?? []);
    const currentIndex = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const nextIndex = Math.max(0, Math.min(buttons.length - 1, currentIndex + direction));
    buttons[nextIndex]?.focus();
  };

  const onRowKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>, row: InventoryRow) => {
    const buttons = Array.from(listRef.current?.querySelectorAll<HTMLButtonElement>("button[data-resource-key]") ?? []);
    if (event.code === "Enter" || event.code === "Space") {
      event.preventDefault();
      onSelectResource(row.resource, row.resourceKey);
    } else if (event.code === "ArrowDown" || event.code === "ArrowRight") {
      event.preventDefault();
      moveFocus(row, 1);
    } else if (event.code === "ArrowUp" || event.code === "ArrowLeft") {
      event.preventDefault();
      moveFocus(row, -1);
    } else if (event.code === "Home") {
      event.preventDefault();
      moveFocus(row, -buttons.length);
    } else if (event.code === "End") {
      event.preventDefault();
      moveFocus(row, buttons.length);
    }
  };

  return (
    <section className="absolute inset-0 flex min-h-0 flex-col bg-[var(--ct-bg)]" aria-labelledby="mesh-inventory-heading" data-testid="mesh-inventory-list">
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-4 py-3">
        <div className="mr-auto flex items-center gap-2">
          <h1 id="mesh-inventory-heading" className="m-0 text-sm font-semibold text-[var(--ct-fg)]">Inventory</h1>
          <span className={UI_TEXT.metaMuted} data-testid="mesh-inventory-count">{model.visible} of {model.total}</span>
        </div>
        <label className="flex min-w-[220px] flex-1 items-center gap-2 sm:max-w-[360px]">
          <Search size={14} className="shrink-0 text-[var(--ct-muted-fg)]" aria-hidden="true" />
          <span className="sr-only">Search inventory</span>
          <input
            ref={searchRef}
            type="search"
            value={filters.search}
            onChange={(event) => updateFilter("search", event.target.value)}
            placeholder="Search inventory"
            aria-label="Search inventory"
            data-testid="mesh-inventory-search"
            className={`${UI_INPUT.base} ${UI_INPUT.compact} min-w-0 flex-1`}
          />
          <kbd className={UI_PANEL.kbd}>/</kbd>
        </label>
        {headerActions && <div className="flex shrink-0 items-center gap-1" data-testid="mesh-inventory-header-actions">{headerActions}</div>}
        {onRefresh && (
          <button type="button" onClick={onRefresh} className={`${UI_BUTTON.ghost} ${UI_BUTTON.icon}`} aria-label="Refresh inventory" data-testid="mesh-inventory-refresh">
            <BusySpinner icon={RefreshCw} size={14} spinning={loading} />
          </button>
        )}
      </div>

      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-[var(--ct-border)] px-4 py-2" aria-label="Inventory filters">
        <SlidersHorizontal size={13} className="text-[var(--ct-muted-fg)]" aria-hidden="true" />
        <FilterSelect label="Machine" value={filters.machine} onChange={(value) => updateFilter("machine", value)} data-testid="mesh-inventory-machine-filter">
          <option value="all">All machines</option>
          {machineOptions(model).map(([id, name]) => <option key={id} value={id}>{name}</option>)}
        </FilterSelect>
        <FilterSelect label="Type" value={filters.kind} onChange={(value) => updateFilter("kind", value)} data-testid="mesh-inventory-kind-filter">
          <option value="all">All types</option>
          <option value="cron">Schedules</option>
          <option value="daemon">Services</option>
        </FilterSelect>
        <FilterSelect label="State" value={filters.state} onChange={(value) => updateFilter("state", value)} data-testid="mesh-inventory-state-filter">
          <option value="all">All states</option>
          {STATE_OPTIONS.map((state) => <option key={state} value={state}>{state}</option>)}
        </FilterSelect>
        <FilterSelect label="Freshness" value={filters.freshness} onChange={(value) => updateFilter("freshness", value)} data-testid="mesh-inventory-freshness-filter">
          <option value="all">Any freshness</option>
          <option value="fresh">Fresh</option>
          <option value="stale">Stale</option>
          <option value="offline">Offline</option>
        </FilterSelect>
        <FilterSelect label="Operations" value={filters.capability} onChange={(value) => updateFilter("capability", value)} data-testid="mesh-inventory-capability-filter">
          <option value="all">Any operations</option>
          <option value="available">Has available operations</option>
          <option value="unavailable">Unavailable / denied</option>
        </FilterSelect>
        {hasFilters(filters) && <button type="button" className={`${UI_BUTTON.ghost} ${UI_BUTTON.sm}`} onClick={clearFilters}>Clear filters</button>}
      </div>

      <div className="min-h-0 flex-1 overflow-auto pb-24" ref={listRef} data-testid="mesh-inventory-scroll-region" style={{ scrollPaddingBottom: "6rem" }}>
        <InventoryStatus model={model} error={error} onClearFilters={clearFilters} />
        {model.peerIssues.map((issue) => (
          <div key={issue.machineId} role="alert" className="border-b border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)]/60 px-4 py-2 text-xs text-[var(--ct-yellow)]">
            {issue.machineName}: inventory {issue.freshness === "offline" ? "offline" : "partially unavailable"}{issue.partialError ? ` — ${issue.partialError}` : ""}. Cached resources remain visible when available.
          </div>
        ))}
        {model.rows.length > 0 && (
          <>
            <div className="sticky top-0 z-[1] grid min-w-[920px] grid-cols-[100px_minmax(150px,1.4fr)_minmax(130px,1fr)_120px_minmax(180px,1.4fr)_130px_100px_minmax(170px,1.5fr)] items-center gap-3 border-b border-[var(--ct-border)] bg-[var(--ct-bg)] px-4 py-2 text-[9px] uppercase tracking-wide text-[var(--ct-muted)]">
              <SortButton label="Kind" sortKey="kind" sort={sort} onSort={toggleSort} />
              <SortButton label="Name" sortKey="name" sort={sort} onSort={toggleSort} />
              <SortButton label="Machine" sortKey="machine" sort={sort} onSort={toggleSort} />
              <SortButton label="State / health" sortKey="state" sort={sort} onSort={toggleSort} />
              <SortButton label="Schedule / next run" sortKey="schedule" sort={sort} onSort={toggleSort} />
              <span>Last run / result</span>
              <span>Freshness</span>
              <span>Available operations</span>
            </div>
            {model.rows.map((row) => (
              <InventoryRowButton
                key={row.resourceKey}
                row={row}
                selected={row.resourceKey === selectedResourceKey}
                onFocus={() => { focusedResourceKeyRef.current = row.resourceKey; }}
                onKeyDown={(event) => onRowKeyDown(event, row)}
                onClick={() => onSelectResource(row.resource, row.resourceKey)}
              />
            ))}
          </>
        )}
      </div>
      <div className="sr-only" aria-live="polite" aria-atomic="true" data-testid="mesh-inventory-announcement">{announcement}</div>
    </section>
  );
}

function machineOptions(model: InventoryModel): Array<[string, string]> {
  const options = new Map<string, string>();
  for (const row of model.allRows) options.set(row.machineId, row.machineName);
  return Array.from(options.entries()).sort((left, right) => left[1].localeCompare(right[1]) || left[0].localeCompare(right[0]));
}

function hasFilters(filters: InventoryFilters): boolean {
  return filters.search !== "" || filters.machine !== "all" || filters.kind !== "all" || filters.state !== "all" || filters.freshness !== "all" || filters.capability !== "all";
}

function FilterSelect<T extends string>({ label, value, onChange, children, ...props }: { label: string; value: T; onChange: (value: T) => void; children: React.ReactNode; "data-testid"?: string }) {
  return (
    <label className="flex items-center gap-1.5">
      <span className="sr-only">{label}</span>
      <select {...props} aria-label={label} value={value} onChange={(event) => onChange(event.target.value as T)} className={`${UI_INPUT.select} text-[10px]`}>
        {children}
      </select>
    </label>
  );
}

function SortButton({ label, sortKey, sort, onSort }: { label: string; sortKey: InventorySortKey; sort: InventorySort; onSort: (key: InventorySortKey) => void }) {
  const active = sort.key === sortKey;
  return (
    <button type="button" className="flex min-w-0 items-center gap-1 text-left text-[9px] uppercase tracking-wide text-[var(--ct-muted)] hover:text-[var(--ct-fg)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--ct-accent)]" onClick={() => onSort(sortKey)} aria-label={`Sort by ${label}`} aria-pressed={active} data-testid={`mesh-inventory-sort-${sortKey}`}>
      <span className="truncate">{label}</span>
      {active && (sort.direction === "asc" ? <ChevronUp size={11} aria-hidden="true" /> : <ChevronDown size={11} aria-hidden="true" />)}
    </button>
  );
}

function InventoryRowButton({ row, selected, onFocus, onKeyDown, onClick }: { row: InventoryRow; selected: boolean; onFocus: () => void; onKeyDown: (event: React.KeyboardEvent<HTMLButtonElement>) => void; onClick: () => void }) {
  const schedule = row.schedule ? `${row.schedule}${row.nextRun == null ? "" : ` · ${formatEpoch(row.nextRun)}`}` : row.nextRun == null ? "—" : formatEpoch(row.nextRun);
  const lastRun = row.lastRunResult ?? "—";
  const label = `${row.kindLabel} ${row.resource.name}, machine ${row.machineName}, state ${row.stateLabel}, schedule ${schedule}, last run ${lastRun}, freshness ${row.freshness}, operations ${row.operationSummary}`;
  return (
    <button
      type="button"
      data-testid="mesh-inventory-row"
      data-resource-key={row.resourceKey}
      data-resource-id={row.resourceId}
      data-machine-id={row.machineId}
      data-kind={row.kind}
      data-freshness={row.freshness}
      data-selected={String(selected)}
      aria-label={label}
      aria-pressed={selected}
      onFocus={onFocus}
      onKeyDown={onKeyDown}
      onClick={onClick}
      className={`grid min-w-[920px] w-full grid-cols-[100px_minmax(150px,1.4fr)_minmax(130px,1fr)_120px_minmax(180px,1.4fr)_130px_100px_minmax(170px,1.5fr)] items-center gap-3 border-b border-[var(--ct-border)] px-4 py-3 text-left text-xs transition-colors hover:bg-[var(--ct-titlebar-bg)] focus:outline-none focus-visible:z-[1] focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--ct-accent)] ${selected ? "bg-[var(--ct-accent)]/10" : ""} ${row.freshness !== "fresh" ? "opacity-75" : ""}`}
    >
      <span className="font-medium text-[var(--ct-fg)]">{row.kindLabel}</span>
      <span className="min-w-0 truncate font-medium text-[var(--ct-fg)]" title={row.resource.name}>{row.resource.name}</span>
      <span className="min-w-0 truncate text-[var(--ct-muted-fg)]" title={`${row.machineName} (${row.machineId})`}>{row.machineName}</span>
      <span className="truncate capitalize text-[var(--ct-fg)]">{row.stateLabel}</span>
      <span className="min-w-0 truncate font-mono text-[10px] text-[var(--ct-muted-fg)]" title={schedule}>{schedule}</span>
      <span className="truncate text-[var(--ct-muted-fg)]">{lastRun}</span>
      <span className={`capitalize ${row.freshness === "fresh" ? "text-[var(--ct-green)]" : "text-[var(--ct-yellow)]"}`}>{row.freshness}</span>
      <span className="flex min-w-0 items-center gap-1 text-[10px] text-[var(--ct-muted-fg)]" title={row.operationSummary}>
        {row.hasAvailableOperation && <Check size={11} className="shrink-0 text-[var(--ct-green)]" aria-hidden="true" />}
        <span className="truncate">{row.operationSummary}</span>
      </span>
    </button>
  );
}

function InventoryStatus({ model, error, onClearFilters }: { model: InventoryModel; error: string | null; onClearFilters: () => void }) {
  if (model.state === "loading") return <div className="p-8 text-center text-sm text-[var(--ct-muted-fg)]" role="status" data-testid="mesh-inventory-loading">Loading inventory…</div>;
  if (model.state === "error") return <div className="p-8 text-center text-sm text-[var(--ct-red)]" role="alert" data-testid="mesh-inventory-error">Inventory unavailable{error ? `: ${error}` : "."}</div>;
  if (model.state === "empty") return <div className="p-8 text-center text-sm text-[var(--ct-muted-fg)]" data-testid="mesh-inventory-empty">No resources registered.</div>;
  if (model.state === "partial-error") return model.total === 0 ? <div className="p-8 text-center text-sm text-[var(--ct-yellow)]" data-testid="mesh-inventory-partial-empty">No resources are available from the current mesh.</div> : <InventoryBanner model={model} error={error} />;
  if (model.state === "filtered-empty") return <div className="p-8 text-center text-sm text-[var(--ct-muted-fg)]" data-testid="mesh-inventory-filtered-empty">No resources match the current filters.<br /><button type="button" className={`${UI_BUTTON.ghost} ${UI_BUTTON.sm} mt-3`} onClick={onClearFilters}>Clear filters</button></div>;
  if (model.state === "loading-cached") return <InventoryBanner model={model} error={error} />;
  if (model.state === "stale" || model.state === "offline") return <InventoryBanner model={model} error={error} />;
  return null;
}

function InventoryBanner({ model, error }: { model: InventoryModel; error?: string | null }) {
  const text = error
    ? `Inventory refresh failed: ${error}. Showing the last known rows.`
    : model.state === "loading-cached"
    ? "Refreshing inventory… showing cached rows."
    : model.state === "partial-error"
      ? "Some peers failed to report. Live rows and cached rows remain available."
      : model.state === "offline"
        ? "Some inventory is offline. Operations are disabled by the target projection."
        : "Some inventory is stale. Operations are disabled until it is fresh.";
  return <div role="status" className="border-b border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-4 py-2 text-xs text-[var(--ct-yellow)]" data-testid="mesh-inventory-status">{text}</div>;
}

function formatEpoch(epoch: number): string {
  return new Date(epoch * 1000).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}
