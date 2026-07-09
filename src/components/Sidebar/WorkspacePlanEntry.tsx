import { useMemo } from "react";
import { Map } from "lucide-react";
import type { Pane, Workspace } from "../../lib/types";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { ActiveTabPill } from "./ActiveTabPill";
import { canvasFocusStore, usePlanPulseVersion } from "../Canvas/canvasFocusStore";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import { useFullPlansOptional } from "../Canvas/PlansProvider";
import { inflightTooltip, selectActiveWork } from "../Canvas/planView/activeWork";
import { workspacePlanTabId } from "./sidebarSelection";
import {
  deriveWorkspacePlanEntries,
  openWorkspacePlanForEntries,
  workspacePlanEntryHint,
  workspacePlanEntryPrimaryTitle,
  workspacePlanEntryTitle,
} from "../Canvas/workspacePlanEntry";

interface WorkspacePlanEntryProps {
  workspaceId: string;
  entries: PlanIndexEntry[];
  isActive: boolean;
  pulsing?: boolean;
  inflightCount?: number;
  inflightTooltip?: string | null;
  liveExecutor?: boolean;
  onClick: () => void;
  touchMode?: boolean;
  compact?: boolean;
  uiFontScale?: number;
}

export function WorkspacePlanEntry({
  workspaceId,
  entries,
  isActive,
  pulsing = false,
  inflightCount = 0,
  inflightTooltip: inflightTip = null,
  liveExecutor = false,
  onClick,
  touchMode = false,
  compact = false,
  uiFontScale,
}: WorkspacePlanEntryProps) {
  const title = workspacePlanEntryTitle(entries);
  const planTitle = workspacePlanEntryPrimaryTitle(entries);
  const hint = workspacePlanEntryHint(entries);
  const tip = inflightTip ? `${title}\n${inflightTip}` : title;
  const rowText = pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  const rowTouchText = pickUiFontScaleValue(uiFontScale, ["text-[13px]", "text-[14px]", "text-[15px]", "text-base"]);
  const iconSize = touchMode ? 14 : compact ? 11 : 12;
  const iconBox = touchMode ? "h-6 w-6" : "h-5 w-5";
  const tabId = workspacePlanTabId(workspaceId);

  if (compact) {
    return (
      <button
        type="button"
        data-testid="workspace-plan-entry"
        data-tab-id={tabId}
        data-tab-kind="workspace-plan"
        data-workspace-action="plan"
        data-active={isActive ? "true" : "false"}
        aria-pressed={isActive}
        aria-current={isActive ? "page" : undefined}
        title={tip}
        onClick={onClick}
        className={`
          sidebar-workspace-plan-compact sidebar-tab-plan relative flex items-center justify-center border-0
          ${isActive ? "bg-[var(--ct-active)] text-[var(--ct-fg)]" : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-muted-fg)]"}
          ${pulsing ? "sidebar-workspace-plan-pulse" : ""}
          ${liveExecutor ? "sidebar-workspace-plan-live" : ""}
        `}
      >
        <Map
          size={iconSize}
          className={isActive ? "text-[var(--ct-accent)]" : "text-[var(--ct-muted)]"}
        />
        {inflightCount > 0 && (
          <span data-testid="sidebar-inflight-badge" className="sidebar-workspace-plan-inflight-badge">
            {inflightCount}
          </span>
        )}
      </button>
    );
  }

  return (
    <button
      type="button"
      data-testid="workspace-plan-entry"
      data-tab-id={tabId}
      data-tab-kind="workspace-plan"
      data-workspace-action="plan"
      data-active={isActive ? "true" : "false"}
      aria-pressed={isActive}
      aria-current={isActive ? "page" : undefined}
      title={tip}
      onClick={onClick}
      className={`
        sidebar-tab sidebar-tab-plan group flex w-full items-center justify-between cursor-pointer transition-colors relative border-0
        ${touchMode ? "sidebar-tab-touch min-h-11 pl-5 pr-2 py-2" : "pl-4 pr-2 py-1.5"}
        ${isActive ? "text-[var(--ct-fg)]" : "sidebar-tab-dimmed hover:bg-[rgba(255,255,255,0.04)]"}
        ${pulsing ? "sidebar-workspace-plan-pulse" : ""}
        ${liveExecutor ? "sidebar-workspace-plan-live" : ""}
      `}
    >
      {isActive && <ActiveTabPill />}
      <div className={`flex flex-1 items-center min-w-0 relative z-[1] ${touchMode ? "gap-2.5" : "gap-2"}`}>
        <span className={`sidebar-workspace-plan-icon shrink-0 ${iconBox}${liveExecutor ? " sidebar-workspace-plan-icon-live" : ""}`}>
          <Map
            size={iconSize}
            className={isActive ? "text-[var(--ct-accent)]" : "text-[var(--ct-muted)]"}
          />
        </span>
        <span className="min-w-0 flex-1 overflow-hidden">
          <span
            data-testid="workspace-plan-title"
            className={`sidebar-workspace-plan-title block truncate whitespace-nowrap ${touchMode ? rowTouchText : rowText} ${isActive ? "font-medium" : ""}`}
          >
            {planTitle}
          </span>
        </span>
        {inflightCount > 0 && (
          <span
            data-testid="sidebar-inflight-badge"
            className="sidebar-workspace-plan-inflight-badge shrink-0"
            title={inflightTip ?? undefined}
          >
            {inflightCount}
          </span>
        )}
        {hint && (
          <span className="sidebar-workspace-plan-hint shrink-0" data-testid="workspace-plan-hint">
            {hint}
          </span>
        )}
      </div>
    </button>
  );
}

export function WorkspacePlanTreeSlot({
  workspace,
  panes,
  planIndex,
  canvasActive,
  touchMode = false,
  compact = false,
  uiFontScale,
  onBeforeOpen,
}: {
  workspace: Workspace;
  panes: Record<string, Pane>;
  planIndex: PlanIndexEntry[];
  canvasActive: boolean;
  touchMode?: boolean;
  compact?: boolean;
  uiFontScale?: number;
  onBeforeOpen?: () => void;
}) {
  const entries = useMemo(
    () => deriveWorkspacePlanEntries(planIndex, workspace, panes),
    [planIndex, workspace, panes],
  );
  const full = useFullPlansOptional();
  const activeWork = useMemo(() => {
    const planIds = new Set(entries.map((e) => e.plan_id));
    const plans = full.plans.filter((p) => planIds.has(p.id));
    return selectActiveWork(plans, panes);
  }, [entries, full.plans, panes]);
  const inflightEntries = activeWork.entries;
  const liveExecutor = inflightEntries.some((e) => e.executorPresent);
  const planIds = useMemo(() => entries.map((e) => e.plan_id), [entries]);
  usePlanPulseVersion(planIds);
  if (entries.length === 0) return null;
  const pulsing = canvasFocusStore.anyPulsing(planIds) && !canvasActive;
  return (
    <WorkspacePlanEntry
      workspaceId={workspace.id}
      entries={entries}
      isActive={canvasActive}
      pulsing={pulsing}
      inflightCount={inflightEntries.length}
      inflightTooltip={inflightEntries.length > 0 ? inflightTooltip(inflightEntries) : null}
      liveExecutor={liveExecutor}
      touchMode={touchMode}
      compact={compact}
      uiFontScale={uiFontScale}
      onClick={() => {
        onBeforeOpen?.();
        openWorkspacePlanForEntries(entries);
      }}
    />
  );
}
