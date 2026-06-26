import { useMemo } from "react";
import { Map } from "lucide-react";
import type { Pane, Workspace } from "../../lib/types";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { canvasFocusStore, usePlanPulseVersion } from "../Canvas/canvasFocusStore";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import {
  deriveWorkspacePlanEntries,
  openWorkspacePlanForEntries,
  workspacePlanEntryKind,
  workspacePlanEntryLabel,
  workspacePlanEntryTitle,
} from "../Canvas/workspacePlanEntry";

interface WorkspacePlanEntryProps {
  entries: PlanIndexEntry[];
  isActive: boolean;
  pulsing?: boolean;
  onClick: () => void;
  touchMode?: boolean;
  compact?: boolean;
  uiFontScale?: number;
}

export function WorkspacePlanEntry({
  entries,
  isActive,
  pulsing = false,
  onClick,
  touchMode = false,
  compact = false,
  uiFontScale,
}: WorkspacePlanEntryProps) {
  const label = workspacePlanEntryLabel(entries);
  const kind = workspacePlanEntryKind(entries);
  const title = workspacePlanEntryTitle(entries);
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
        title={title}
        onClick={onClick}
        className={`
          sidebar-workspace-plan sidebar-workspace-plan-compact border-0
          ${pulsing ? "sidebar-workspace-plan-pulse" : ""}
        `}
      >
        <Map
          size={iconSize}
          className={isActive ? "text-[var(--ct-accent)]" : "text-[var(--ct-muted)]"}
        />
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
      title={title}
      onClick={onClick}
      className={`
        sidebar-workspace-plan w-full border-0
        ${touchMode ? "sidebar-workspace-plan-touch" : ""}
        ${pulsing ? "sidebar-workspace-plan-pulse" : ""}
      `}
    >
      <div className={`flex flex-1 items-center min-w-0 gap-2 ${touchMode ? "gap-2.5" : ""}`}>
        <span className={`sidebar-workspace-plan-icon ${iconBox}`}>
          <Map
            size={iconSize}
            className={isActive ? "text-[var(--ct-accent)]" : "text-[var(--ct-muted)]"}
          />
        </span>
        <span className={`sidebar-workspace-plan-kind ${touchMode ? "text-[10px]" : ""}`}>
          {kind}
        </span>
        {label !== kind && (
          <>
            <span className="text-[var(--ct-muted)] shrink-0" aria-hidden>
              ·
            </span>
            <span className={`${touchMode ? rowTouchText : rowText} truncate ${isActive ? "font-medium" : ""}`}>
              {label}
            </span>
          </>
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
  const planIds = useMemo(() => entries.map((e) => e.plan_id), [entries]);
  usePlanPulseVersion(planIds);
  if (entries.length === 0) return null;
  const pulsing = canvasFocusStore.anyPulsing(planIds) && !canvasActive;
  return (
    <WorkspacePlanEntry
      entries={entries}
      isActive={canvasActive}
      pulsing={pulsing}
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
