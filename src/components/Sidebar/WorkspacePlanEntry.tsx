import { useMemo } from "react";
import { Map } from "lucide-react";
import type { Pane, Workspace } from "../../lib/types";
import { pickUiFontScaleValue } from "../../lib/uiFontScale";
import { canvasFocusStore, usePlanPulseVersion } from "../Canvas/canvasFocusStore";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import {
  deriveWorkspacePlanEntries,
  openWorkspacePlanForEntries,
  workspacePlanEntryLabel,
} from "../Canvas/workspacePlanEntry";

interface WorkspacePlanEntryProps {
  entries: PlanIndexEntry[];
  isActive: boolean;
  pulsing?: boolean;
  onClick: () => void;
  touchMode?: boolean;
  uiFontScale?: number;
}

export function WorkspacePlanEntry({
  entries,
  isActive,
  pulsing = false,
  onClick,
  touchMode = false,
  uiFontScale,
}: WorkspacePlanEntryProps) {
  const label = workspacePlanEntryLabel(entries);
  const rowText = pickUiFontScaleValue(uiFontScale, ["text-[11px]", "text-xs", "text-sm", "text-base"]);
  const rowTouchText = pickUiFontScaleValue(uiFontScale, ["text-[13px]", "text-[14px]", "text-[15px]", "text-base"]);

  return (
    <button
      type="button"
      data-testid="workspace-plan-entry"
      title="Open workspace plan canvas"
      onClick={onClick}
      className={`
        sidebar-tab w-full flex items-center cursor-pointer transition-colors relative border-0 bg-transparent text-left
        ${touchMode ? "sidebar-tab-touch min-h-11 pl-5 pr-2 py-2" : "pl-4 pr-2 py-1.5"}
        ${pulsing ? "ct-plan-button-pulse" : ""}
        ${isActive ? "text-[var(--ct-fg)]" : "sidebar-tab-dimmed hover:bg-[rgba(255,255,255,0.04)]"}
      `}
    >
      {isActive && (
        <div
          className="absolute inset-y-0 left-0 right-0 pointer-events-none"
          style={{
            background: "var(--ct-bg-selected, rgba(99, 102, 241, 0.12))",
            transition: "transform 320ms cubic-bezier(0.16, 1, 0.3, 1), opacity 200ms ease",
          }}
        />
      )}
      <div className={`flex flex-1 items-center min-w-0 relative z-[1] ${touchMode ? "gap-2.5" : "gap-2"}`}>
        <Map
          size={touchMode ? 15 : 12}
          className={`shrink-0 ${isActive ? "text-[var(--ct-accent)]" : "text-[var(--ct-muted)]"}`}
        />
        <span className={`${touchMode ? rowTouchText : rowText} truncate ${isActive ? "font-medium" : ""}`}>
          {label}
        </span>
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
  uiFontScale,
  onBeforeOpen,
}: {
  workspace: Workspace;
  panes: Record<string, Pane>;
  planIndex: PlanIndexEntry[];
  canvasActive: boolean;
  touchMode?: boolean;
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
      uiFontScale={uiFontScale}
      onClick={() => {
        onBeforeOpen?.();
        openWorkspacePlanForEntries(entries);
      }}
    />
  );
}
