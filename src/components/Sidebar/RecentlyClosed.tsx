import { memo, useState, useCallback } from "react";
import { createPortal } from "react-dom";
import { RotateCcw, ChevronRight, GitBranch, Layers } from "lucide-react";
import type { ClosedPaneView } from "../../lib/types";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";

/** A render group collapsed from the flat closed-history view. A `single`
 *  close is one pane; `cascade` is an orchestrator subtree; `workspace` is a
 *  whole closed workspace. */
export interface ClosedGroup {
  id: string;
  kind: "single" | "cascade" | "workspace";
  panes: ClosedPaneView[];
}

/** Collapse the flat newest-first view into render groups, preserving server
 *  order by each group's first appearance. */
export function groupClosedViews(views: ClosedPaneView[]): ClosedGroup[] {
  const order: string[] = [];
  const byId = new Map<string, ClosedGroup>();
  for (const v of views) {
    let g = byId.get(v.group_id);
    if (!g) {
      g = { id: v.group_id, kind: v.group_kind, panes: [] };
      byId.set(v.group_id, g);
      order.push(v.group_id);
    }
    g.panes.push(v);
  }
  return order.map((id) => byId.get(id) as ClosedGroup);
}

/** Compact relative age from a `closed_at` unix-millis stamp: `now`, `5m`,
 *  `3h`, `2d`, then an absolute date. Empty for a missing/zero stamp. */
export function closedAgo(closedAtMs: number, nowMs: number = Date.now()): string {
  if (!closedAtMs) return "";
  const s = Math.max(0, Math.round((nowMs - closedAtMs) / 1000));
  if (s < 45) return "now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h`;
  const d = Math.round(h / 24);
  if (d < 7) return `${d}d`;
  return new Date(closedAtMs).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** Newest `closed_at` within a group (panes aren't guaranteed ordered). */
function groupClosedAt(group: ClosedGroup): number {
  return group.panes.reduce((max, p) => Math.max(max, p.closed_at ?? 0), 0);
}

/** Header label for a multi-pane group — just the workspace name or the
 *  orchestrator's title. The pane count lives in the adjacent count pill, so
 *  it's deliberately left out of the text. */
export function describeGroup(group: ClosedGroup): string {
  if (group.kind === "workspace") {
    return group.panes.find((p) => p.workspace_name)?.workspace_name ?? "Workspace";
  }
  const lead = group.panes.find((p) => !p.parent_pane_id) ?? group.panes[0];
  return lead.title || "Orchestrator";
}

interface RecentlyClosedProps {
  /** Flat newest-first closed-history view; grouped here for display. */
  views: ClosedPaneView[];
  showWorkspace: boolean;
  /** Reopen a single pane (or one pane inside a group). */
  onReopen: (workspaceId: string, index: number, orphaned: boolean, paneId: string) => void;
  /** Reopen an entire cascade/workspace group at once. */
  onReopenGroup: (groupId: string) => void;
  touchMode?: boolean;
  uiFontScale?: number;
}

interface TooltipState {
  text: string;
  x: number;
  y: number;
}

function RecentlyClosedInner({
  views,
  showWorkspace,
  onReopen,
  onReopenGroup,
  touchMode = false,
  uiFontScale,
}: RecentlyClosedProps) {
  const [open, setOpen] = useState(false);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [tooltip, setTooltip] = useState<TooltipState | null>(null);

  const handleMouseMove = useCallback((text: string, e: React.MouseEvent) => {
    setTooltip({ text, x: e.clientX, y: e.clientY });
  }, []);

  const handleMouseLeave = useCallback(() => {
    setTooltip(null);
  }, []);

  const toggleGroup = useCallback((id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  if (views.length === 0) return null;

  const groups = groupClosedViews(views);
  const text = pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  const metaText = pickUiFontScaleValue(uiFontScale, ["text-[9px]", "text-[10px]", "text-[11px]", "text-xs"]);
  const rowPad = touchMode ? "py-2" : "py-1";

  const paneRow = (view: ClosedPaneView, child: boolean) => (
    <button
      key={`${view.group_id}-${view.pane_id}-${view.index}`}
      data-testid="recently-closed-item"
      onClick={() => {
        setTooltip(null);
        onReopen(view.workspace_id ?? "", view.index, view.orphaned, view.pane_id);
      }}
      onMouseMove={view.description ? (e) => handleMouseMove(view.description as string, e) : undefined}
      onMouseLeave={view.description ? handleMouseLeave : undefined}
      className={`flex items-center gap-2 w-full ${child ? "pl-9" : "pl-6"} pr-2 ${rowPad} rounded-md text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors`}
    >
      {child && (
        <span className="shrink-0 -ml-3 mr-0.5 text-[var(--ct-border)] select-none" aria-hidden>
          └
        </span>
      )}
      <span className={`${text} truncate min-w-0 flex-1 text-left`}>{view.title || "Untitled"}</span>
      {showWorkspace && view.workspace_name && (
        <span className={`${metaText} shrink-0 text-[var(--ct-muted)] truncate max-w-[60px]`}>
          {view.workspace_name}
        </span>
      )}
      {view.model && (
        <span className={`${metaText} shrink-0 px-1 rounded bg-[var(--ct-active)] text-[var(--ct-muted)]`}>
          {view.model}
        </span>
      )}
      {view.closed_at ? (
        <span className={`${metaText} shrink-0 text-[var(--ct-muted)] tabular-nums w-7 text-right`}>
          {closedAgo(view.closed_at)}
        </span>
      ) : null}
    </button>
  );

  return (
    <div data-testid="recently-closed" className="mx-2 mt-0.5 mb-1">
      <button
        onClick={() => setOpen((v) => !v)}
        className={`flex items-center gap-1 w-full px-2 ${touchMode ? "py-2" : "py-1"} rounded-md text-[var(--ct-muted)] hover:text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] transition-colors`}
        title="Recently closed tabs"
      >
        <ChevronRight
          size={10}
          strokeWidth={1.5}
          className="shrink-0 transition-transform duration-200"
          style={{ transform: open ? "rotate(90deg)" : "rotate(0deg)" }}
        />
        <RotateCcw size={10} strokeWidth={1.5} className="shrink-0" />
        <span className={`${metaText} font-medium uppercase tracking-wide`}>Recently closed</span>
      </button>
      {open && (
        <div className="mt-0.5 overflow-y-auto" style={{ maxHeight: 125 }}>
          {groups.map((group) => {
            if (group.kind === "single") return paneRow(group.panes[0], false);

            const isOpen = expanded.has(group.id);
            const KindIcon = group.kind === "workspace" ? Layers : GitBranch;
            const label = describeGroup(group);
            return (
              <div key={group.id} data-testid="recently-closed-group">
                <div
                  className={`group/grp flex items-center w-full pr-2 rounded-md text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] transition-colors`}
                >
                  <button
                    data-testid="recently-closed-group-toggle"
                    onClick={() => toggleGroup(group.id)}
                    aria-expanded={isOpen}
                    aria-label={isOpen ? "Collapse group" : "Expand group"}
                    className={`flex items-center justify-center shrink-0 pl-2 pr-1 ${rowPad} text-[var(--ct-muted)] hover:text-[var(--ct-fg)]`}
                  >
                    <ChevronRight
                      size={11}
                      strokeWidth={1.75}
                      className="transition-transform duration-200"
                      style={{ transform: isOpen ? "rotate(90deg)" : "rotate(0deg)" }}
                    />
                  </button>
                  <button
                    data-testid="recently-closed-group-restore"
                    onClick={() => {
                      setTooltip(null);
                      onReopenGroup(group.id);
                    }}
                    title="Restore all"
                    className={`flex items-center gap-2 min-w-0 flex-1 pr-1 ${rowPad} text-left hover:text-[var(--ct-fg)]`}
                  >
                    <KindIcon size={12} strokeWidth={1.75} className="shrink-0 text-[var(--ct-accent)]" />
                    <span className={`${text} font-medium truncate min-w-0 flex-1`}>{label}</span>
                    <span
                      className={`${metaText} shrink-0 px-1.5 rounded-full border border-[var(--ct-border)] text-[var(--ct-muted)] tabular-nums`}
                    >
                      {group.panes.length}
                    </span>
                    <span className={`${metaText} shrink-0 text-[var(--ct-muted)] tabular-nums w-7 text-right`}>
                      {closedAgo(groupClosedAt(group))}
                    </span>
                  </button>
                </div>
                {isOpen && group.panes.map((pane) => paneRow(pane, true))}
              </div>
            );
          })}
        </div>
      )}
      {tooltip &&
        createPortal(
          <div
            className="fixed z-[9999] pointer-events-none px-2 py-1 rounded text-[11px] text-[var(--ct-fg)] bg-[var(--ct-surface)] border border-[var(--ct-border)] shadow-lg max-w-[240px] break-words"
            style={{ left: tooltip.x + 14, top: tooltip.y - 8 }}
          >
            {tooltip.text}
          </div>,
          document.body,
        )}
    </div>
  );
}

export const RecentlyClosed = memo(RecentlyClosedInner);
