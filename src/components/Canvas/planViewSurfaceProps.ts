import type { ApiConfig, Tab, Team, Workspace } from "../../lib/types";
import type { CanvasWorkspaceTarget } from "./canvasFocusStore";
import type { Plan } from "./types";

/** Shared read-only context for Spec | Plan | Board and parked plan surfaces. */
export interface PlanViewSurfaceProps {
  mode: "workspace" | "focus";
  plans: Plan[];
  loading?: boolean;
  reconnecting?: boolean;
  error?: string | null;
  onRetry?: () => void;
  planId?: string;
  teamId?: string;
  orchestratorTabId?: string;
  focusTabId?: string | null;
  tabs?: Record<string, Tab>;
  workspaces?: Workspace[];
  /** Current workspace; workspace mode scopes plans to those bound here. */
  workspaceId?: string;
  teams?: Team[];
  projectPath?: string | null;
  workspaceTarget?: CanvasWorkspaceTarget | null;
  onClose?: () => void;
  onOpenTab?: (tabId: string) => void;
  api?: ApiConfig;
  /** Host header element where a surface without its own switcher places its actions. */
  headerActionsSlot?: HTMLElement | null;
}
