import type { PlanViewSurfaceProps } from "../planViewSurfaceProps";

export function PlanBoardView({
  plans,
  loading = false,
  error = null,
  mode,
  planId,
}: PlanViewSurfaceProps) {
  const title =
    mode === "focus" && planId
      ? plans.find((p) => p.id === planId)?.title ?? "Plan"
      : plans.length === 1
        ? plans[0]?.title ?? "Plans"
        : `${plans.length} plans`;

  return (
    <div
      data-testid="plan-board-view"
      className="flex h-full w-full flex-col overflow-hidden"
      style={{ background: "var(--ct-bg)", color: "var(--ct-fg)" }}
    >
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 px-6 text-center">
        {loading && (
          <p className="text-sm" style={{ opacity: "var(--ct-text-secondary-opacity)" }}>
            Loading plans…
          </p>
        )}
        {!loading && error && (
          <p className="text-sm" style={{ color: "var(--ct-error, #f87171)" }}>
            {error}
          </p>
        )}
        {!loading && !error && (
          <>
            <p className="text-[11px] font-medium uppercase tracking-wide" style={{ color: "var(--ct-muted-fg)" }}>
              Board
            </p>
            <p className="max-w-md text-sm font-medium">{title}</p>
            <p className="max-w-md text-xs" style={{ color: "var(--ct-muted-fg)" }}>
              Read-only kanban board for agent task flow. Full board view plugs in here.
            </p>
          </>
        )}
      </div>
    </div>
  );
}
