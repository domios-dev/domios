import { memo, useState, useCallback } from "react";
import { createPortal } from "react-dom";
import { GitBranch, Layers } from "lucide-react";
import type { ClosedTabView } from "../../lib/types";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { formatModelBadge } from "../../lib/formatModelBadge";
import { SidebarModelBadge } from "./SidebarModelBadge";
import { SidebarDisclosure } from "./SidebarDisclosure";

/** A render group collapsed from the flat closed-history view. A `single`
 *  close is one tab; `cascade` is an orchestrator subtree; `workspace` is a
 *  whole closed workspace. */
export interface ClosedGroup {
  id: string;
  kind: "single" | "cascade" | "workspace";
  tabs: ClosedTabView[];
}

/** Collapse the flat newest-first view into render groups, preserving server
 *  order by each group's first appearance. */
export function groupClosedViews(views: ClosedTabView[]): ClosedGroup[] {
  const order: string[] = [];
  const byId = new Map<string, ClosedGroup>();
  for (const v of views) {
    let g = byId.get(v.group_id);
    if (!g) {
      g = { id: v.group_id, kind: v.group_kind, tabs: [] };
      byId.set(v.group_id, g);
      order.push(v.group_id);
    }
    g.tabs.push(v);
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

/** Newest `closed_at` within a group (tabs aren't guaranteed ordered). */
function groupClosedAt(group: ClosedGroup): number {
  return group.tabs.reduce((max, p) => Math.max(max, p.closed_at ?? 0), 0);
}

/** Header label for a multi-tab group — just the workspace name or the
 *  orchestrator's title. The tab count lives in the adjacent count pill, so
 *  it's deliberately left out of the text. */
export function describeGroup(group: ClosedGroup): string {
  if (group.kind === "workspace") {
    return group.tabs.find((p) => p.workspace_name)?.workspace_name ?? "Workspace";
  }
  const lead = group.tabs.find((p) => !p.parent_tab_id) ?? group.tabs[0];
  return lead.title || "Orchestrator";
}

interface RecentlyClosedProps {
  /** Flat newest-first closed-history view; grouped here for display. */
  views: ClosedTabView[];
  showWorkspace: boolean;
  /** Reopen a single tab (or one tab inside a group). */
  onReopen: (workspaceId: string, index: number, orphaned: boolean, tabId: string) => void;
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

  const groups = groupClosedViews(views);
  const text = pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  const metaText = pickUiFontScaleValue(uiFontScale, ["text-[9px]", "text-[10px]", "text-[11px]", "text-xs"]);
  const rowPad = touchMode ? "py-2" : "py-1.5";

  const tabRow = (view: ClosedTabView, child: boolean) => {
    const modelBadge = view.model ? formatModelBadge(view.model) : null;
    const hasMeta = (showWorkspace && view.workspace_name) || view.closed_at;

    return (
      <button
        key={`${view.group_id}-${view.tab_id}-${view.index}`}
        data-testid="recently-closed-item"
        onClick={() => {
          setTooltip(null);
          onReopen(view.workspace_id ?? "", view.index, view.orphaned, view.tab_id);
        }}
        onMouseMove={view.description ? (e) => handleMouseMove(view.description as string, e) : undefined}
        onMouseLeave={view.description ? handleMouseLeave : undefined}
        className={`flex items-start gap-2 w-full ${child ? "pl-6" : "pl-2"} pr-2 ${rowPad} rounded-md text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors`}
      >
        {child && (
          <span className="shrink-0 -ml-2 mr-0.5 mt-0.5 text-[var(--ct-border)] select-none" aria-hidden>
            └
          </span>
        )}
        <span className="min-w-0 flex-1 text-left">
          <span className="flex min-w-0 items-center gap-1.5">
            <span className={`${text} min-w-0 flex-1 truncate leading-tight`}>{view.title || "Untitled"}</span>
            {modelBadge && (
              <SidebarModelBadge
                label={modelBadge}
                title={view.model ?? undefined}
                className="shrink-0 max-w-[92px]"
              />
            )}
          </span>
          {hasMeta && (
            <span className="mt-0.5 flex min-w-0 items-center justify-between gap-2 text-[var(--ct-muted)]">
              {showWorkspace && view.workspace_name ? (
                <span className={`${metaText} min-w-0 flex-1 truncate`}>
                  {view.workspace_name}
                </span>
              ) : (
                <span className="min-w-0 flex-1" />
              )}
              {view.closed_at ? (
                <span className={`${metaText} shrink-0 tabular-nums text-right`}>
                  {closedAgo(view.closed_at)}
                </span>
              ) : null}
            </span>
          )}
        </span>
      </button>
    );
  };

  return (
    <div data-testid="recently-closed" className="sidebar-recently-closed">
      <SidebarDisclosure
        expanded={open}
        touchMode={touchMode}
        onClick={() => setOpen((v) => !v)}
        className="sidebar-structural-header w-full"
        title="Recently closed tabs"
      >
        <span className="sidebar-structural-label truncate">RECENTLY CLOSED</span>
      </SidebarDisclosure>
      {open && (
        <div className="mt-0.5 overflow-y-auto" style={{ maxHeight: 165 }}>
          {groups.length === 0 && (
            <div
              data-testid="recently-closed-empty"
              className={`${metaText} px-2 py-1.5 text-[var(--ct-muted)]`}
            >
              No closed tabs
            </div>
          )}
          {groups.map((group) => {
            if (group.kind === "single") return tabRow(group.tabs[0], false);

            const isOpen = expanded.has(group.id);
            const KindIcon = group.kind === "workspace" ? Layers : GitBranch;
            const label = describeGroup(group);
            return (
              <div key={group.id} data-testid="recently-closed-group">
                <div
                  className={`group/grp flex items-center w-full pr-2 rounded-md text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] transition-colors`}
                >
                  <SidebarDisclosure
                    data-testid="recently-closed-group-toggle"
                    onClick={() => toggleGroup(group.id)}
                    expanded={isOpen}
                    touchMode={touchMode}
                    aria-label={isOpen ? "Collapse group" : "Expand group"}
                    className={`flex items-start justify-center shrink-0 pl-2 pr-1 ${rowPad} text-[var(--ct-muted)] hover:text-[var(--ct-fg)]`}
                  />
                  <button
                    data-testid="recently-closed-group-restore"
                    onClick={() => {
                      setTooltip(null);
                      onReopenGroup(group.id);
                    }}
                    title="Restore all"
                    className={`flex items-start gap-2 min-w-0 flex-1 pr-1 ${rowPad} text-left hover:text-[var(--ct-fg)]`}
                  >
                    <KindIcon size={12} strokeWidth={1.75} className="mt-0.5 shrink-0 text-[var(--ct-accent)]" />
                    <span className="min-w-0 flex-1">
                      <span className={`${text} block font-medium truncate leading-tight`}>{label}</span>
                      <span className="mt-0.5 flex min-w-0 items-center justify-between gap-2 text-[var(--ct-muted)]">
                        <span
                          className={`${metaText} shrink-0 px-1.5 rounded-full border border-[var(--ct-border)] tabular-nums`}
                        >
                          {group.tabs.length}
                        </span>
                        <span className={`${metaText} shrink-0 tabular-nums text-right`}>
                          {closedAgo(groupClosedAt(group))}
                        </span>
                      </span>
                    </span>
                  </button>
                </div>
                {isOpen && group.tabs.map((tab) => tabRow(tab, true))}
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
