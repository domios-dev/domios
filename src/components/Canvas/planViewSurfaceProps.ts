import type { ApiConfig, Pane, Workspace } from "../../lib/types";
import type { CanvasWorkspaceTarget } from "./canvasFocusStore";
import type { Plan } from "./types";

/** Shared read-only context for Canvas | List | Board plan surfaces. */
export interface PlanViewSurfaceProps {
  mode: "workspace" | "focus";
  plans: Plan[];
  loading?: boolean;
  reconnecting?: boolean;
  error?: string | null;
  onRetry?: () => void;
  planId?: string;
  orchestratorPaneId?: string;
  focusPaneId?: string | null;
  panes?: Record<string, Pane>;
  workspaces?: Workspace[];
  focusedPaneId?: string | null;
  projectPath?: string | null;
  workspaceTarget?: CanvasWorkspaceTarget | null;
  onClose?: () => void;
  onOpenPane?: (paneId: string) => void;
  api?: ApiConfig;
  /** All-plans mode lifts the workspace filter (canvas flow surface). */
  allPlans?: boolean;
  onAllPlansChange?: (allPlans: boolean) => void;
}
