import { useMemo } from "react";
import { Map } from "lucide-react";
import type { Pane, Workspace } from "../../lib/types";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { canvasFocusStore, usePlanPulseVersion } from "../Canvas/canvasFocusStore";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import { useFullPlansOptional } from "../Canvas/PlansProvider";
import { inflightTooltip, selectActiveWork } from "../Canvas/planView/activeWork";
import {
  deriveWorkspacePlanEntries,
  openWorkspacePlanForEntries,
  workspacePlanEntryHint,
  workspacePlanEntryPrimaryTitle,
  workspacePlanEntryTitle,
} from "../Canvas/workspacePlanEntry";

interface WorkspacePlanEntryProps {
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
  const iconBox = touchMode ? "h-6 w-6" : compact ? "h-5 w-5" : "h-5 w-5";

  if (compact) {
    return (
      <button
        type="button"
        data-testid="workspace-plan-entry"
        data-workspace-action="plan"
        data-pinned="true"
        data-active={isActive ? "true" : "false"}
        aria-pressed={isActive}
        title={tip}
        onClick={onClick}
        className={`
          sidebar-workspace-plan sidebar-workspace-plan-compact border-0
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
      data-workspace-action="plan"
      data-pinned="true"
      data-active={isActive ? "true" : "false"}
      aria-pressed={isActive}
      title={tip}
      onClick={onClick}
      className={`
        sidebar-workspace-plan w-full border-0
        ${touchMode ? "sidebar-workspace-plan-touch" : ""}
        ${pulsing ? "sidebar-workspace-plan-pulse" : ""}
        ${liveExecutor ? "sidebar-workspace-plan-live" : ""}
      `}
    >
      <div className={`flex flex-1 items-center min-w-0 gap-2 ${touchMode ? "gap-2.5" : ""}`}>
        <span className={`sidebar-workspace-plan-icon ${iconBox}${liveExecutor ? " sidebar-workspace-plan-icon-live" : ""}`}>
          <Map
            size={iconSize}
            className={isActive ? "text-[var(--ct-accent)]" : "text-[var(--ct-muted)]"}
          />
        </span>
        <span
          data-testid="workspace-plan-title"
          className={`sidebar-workspace-plan-title flex-1 min-w-0 truncate ${touchMode ? rowTouchText : rowText} ${isActive ? "font-medium" : ""}`}
        >
          {planTitle}
        </span>
        {inflightCount > 0 && (
          <span
            data-testid="sidebar-inflight-badge"
            className="sidebar-workspace-plan-inflight-badge"
            title={inflightTip ?? undefined}
          >
            {inflightCount}
          </span>
        )}
        {hint && (
          <span className="sidebar-workspace-plan-hint" data-testid="workspace-plan-hint">
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
