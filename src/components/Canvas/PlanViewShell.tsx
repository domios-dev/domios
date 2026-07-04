import { memo, useCallback, useState } from "react";
import { X } from "lucide-react";
import { PlanViewModeSwitch } from "./PlanViewModeSwitch";
import type { PlanViewSurfaceProps } from "./planViewSurfaceProps";
import { MissionControlPlanSurface } from "./missionControl/MissionControlPlanSurface";
import {
  DEFAULT_PLAN_VIEW_MODE,
  resolvePlanViewMode,
  type PlanViewMode,
} from "./planViewMode";
import { PlanBoardView } from "./planView/PlanBoardView";
import { PlanListView } from "./planView/PlanListView";
import { PlanCompanionBubble } from "./planView/PlanCompanionBubble";
import { PaneSessionChat } from "../Terminal/PaneSessionChat";

// All three surfaces stay mounted (toggled via `hidden`) to preserve their
// state; memoizing keeps a pure view-mode switch from re-rendering the two
// off-screen trees, which was a >250ms longtask source under load.
const BoardSurface = memo(PlanBoardView);
const ListSurface = memo(PlanListView);
const CanvasSurface = memo(MissionControlPlanSurface);
const CompanionSurface = memo(PlanCompanionBubble);

const PLAN_VIEW_MODE_STORAGE_KEY = "codeterm.planViewMode";

function readPersistedPlanViewMode(): PlanViewMode {
  try {
    return resolvePlanViewMode(window.localStorage.getItem(PLAN_VIEW_MODE_STORAGE_KEY));
  } catch {
    return DEFAULT_PLAN_VIEW_MODE;
  }
}

function usePersistedPlanViewMode(): [PlanViewMode, (mode: PlanViewMode) => void] {
  const [viewMode, setViewModeState] = useState<PlanViewMode>(readPersistedPlanViewMode);

  const setViewMode = useCallback((mode: PlanViewMode) => {
    setViewModeState(mode);
    try {
      window.localStorage.setItem(PLAN_VIEW_MODE_STORAGE_KEY, mode);
    } catch {
      // Persistence is best-effort; quota/private-mode failures must not block switching.
    }
  }, []);

  return [viewMode, setViewMode];
}

export function PlanViewShell(props: PlanViewSurfaceProps) {
  const [viewMode, setViewMode] = usePersistedPlanViewMode();

  const renderCompanionChat = useCallback(
    ({ paneId, chatBackend }: { paneId: string; chatBackend: boolean }) => (
      <PaneSessionChat paneId={paneId} chatBackend={chatBackend} isVisible />
    ),
    [],
  );

  return (
    <div
      data-testid="plan-view-shell"
      data-plan-view-mode={viewMode}
      className="flex h-full w-full flex-col overflow-hidden"
    >
      <header
        className="flex shrink-0 items-center justify-between gap-3 border-b px-3 py-2"
        style={{
          borderColor: "var(--ct-border)",
          background: "var(--ct-titlebar-bg)",
        }}
      >
        <PlanViewModeSwitch value={viewMode} onChange={setViewMode} />
        {props.onClose && (
          <button
            type="button"
            data-testid="plan-view-close"
            onClick={props.onClose}
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded border cursor-pointer"
            style={{ background: "var(--ct-bg)", borderColor: "var(--ct-border)", color: "var(--ct-fg)" }}
            title="Close plan"
          >
            <X size={14} />
          </button>
        )}
      </header>
      <div className="relative min-h-0 flex-1">
        <div
          className="absolute inset-0"
          hidden={viewMode !== "canvas"}
          aria-hidden={viewMode !== "canvas"}
        >
          <CanvasSurface {...props} />
        </div>
        <div
          className="absolute inset-0"
          hidden={viewMode !== "list"}
          aria-hidden={viewMode !== "list"}
        >
          <ListSurface {...props} onClose={undefined} />
        </div>
        <div
          className="absolute inset-0"
          hidden={viewMode !== "board"}
          aria-hidden={viewMode !== "board"}
        >
          <BoardSurface {...props} />
        </div>
        <CompanionSurface
          plans={props.plans}
          panes={props.panes ?? {}}
          planId={props.planId}
          orchestratorPaneId={props.orchestratorPaneId}
          onOpenPane={props.onOpenPane}
          renderChat={renderCompanionChat}
        />
      </div>
    </div>
  );
}
