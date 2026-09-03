import { PLAN_LIST_STATUS_FILTERS, type PlanListStatusFilter } from "./planListModel";

export interface PlanListFilterBarProps {
  statusFilter: PlanListStatusFilter;
  textQuery: string;
  cancelledCount?: number;
  onStatusFilter: (filter: PlanListStatusFilter) => void;
  onTextQuery: (query: string) => void;
}

export function PlanListFilterBar({
  statusFilter,
  textQuery,
  cancelledCount = 0,
  onStatusFilter,
  onTextQuery,
}: PlanListFilterBarProps) {
  const cancelledActive = statusFilter === "cancelled";

  return (
    <div
      data-testid="plan-list-filter"
      className="flex shrink-0 flex-wrap items-center gap-2 border-b px-3 py-1.5"
      style={{
        borderColor: "color-mix(in srgb, var(--ct-border) 55%, transparent)",
        background: "color-mix(in srgb, var(--ct-bg-elev, var(--ct-bg)) 96%, transparent)",
      }}
    >
      <div className="flex flex-wrap items-center gap-1" role="group" aria-label="Status filter">
        {PLAN_LIST_STATUS_FILTERS.map(({ key, label }) => {
          const active = statusFilter === key;
          return (
            <button
              key={key}
              type="button"
              data-testid={`plan-list-filter-status-${key}`}
              aria-pressed={active}
              className="rounded px-2 py-0.5 text-[10px] font-medium tracking-wide cursor-pointer"
              style={{
                color: active ? "var(--ct-fg)" : "var(--ct-muted-fg)",
                background: active ? "var(--ct-bg-active)" : "transparent",
                border: `1px solid ${active ? "var(--ct-border)" : "transparent"}`,
              }}
              onClick={() => onStatusFilter(key)}
            >
              {label}
            </button>
          );
        })}
        {cancelledCount > 0 && (
          <button
            type="button"
            data-testid="plan-list-filter-status-cancelled"
            aria-pressed={cancelledActive}
            className="rounded px-2 py-0.5 text-[10px] font-medium tracking-wide cursor-pointer"
            style={{
              color: cancelledActive ? "var(--ct-fg)" : "var(--ct-muted-fg)",
              background: cancelledActive ? "var(--ct-bg-active)" : "transparent",
              border: `1px solid ${cancelledActive ? "var(--ct-border)" : "transparent"}`,
            }}
            onClick={() => onStatusFilter("cancelled")}
          >
            Cancelled ({cancelledCount})
          </button>
        )}
      </div>
      <input
        type="search"
        data-testid="plan-list-filter-text"
        aria-label="Filter rows"
        value={textQuery}
        onChange={(e) => onTextQuery(e.target.value)}
        placeholder="Filter rows…"
        className="min-w-[8rem] flex-1 rounded border px-2 py-0.5 text-[11px] outline-none focus-visible:ring-2 focus-visible:ring-[var(--ct-accent)]"
        style={{
          maxWidth: "14rem",
          borderColor: "color-mix(in srgb, var(--ct-border) 70%, transparent)",
          background: "var(--ct-bg)",
          color: "var(--ct-fg)",
        }}
      />
    </div>
  );
}
