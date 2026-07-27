import { createContext, useContext } from "react";
import { TitleLevel } from "../../lib/types";
import type {
  Notification,
  PaneStatus,
  OrchestrationPairInput,
  OrchestrationPairResult,
} from "../../lib/types";
import type { MuxStateStore } from "./selectorStore";

/** Ids returned by a successful spawn — enough to resolve + select the tab. */
export interface SpawnedPane {
  workspaceId: string;
  paneId: string;
  tabId?: string;
}

export interface MultiplexerActionsContextType {
  createWorkspace: (name: string, cwd?: string, opts?: { activate?: boolean }) => Promise<{ id: string; name: string } | undefined>;
  setActiveWorkspace: (id: string) => void;
  closeWorkspace: (id: string) => void;
  renameWorkspace: (id: string, name: string) => void;
  movepane: (tabId: string, fromWorkspaceId: string, toWorkspaceId: string) => void;
  /** Reorder a tab group within its hierarchy level. `parentPaneId` set →
   *  reorder children of that parent; otherwise reorder a workspace's top-level
   *  tabs. Within-group only — the order list never crosses groups. */
  reorderTabs: (args: { workspaceId?: string; parentPaneId?: string; orderedTabIds: string[] }) => void;
  /** Reorder the workspace list to the given order. */
  reorderWorkspaces: (orderedWorkspaceIds: string[]) => void;
  createpane: (workspaceId: string, cwd?: string) => Promise<{ id: string; pane_id: string } | void>;
  closepane: (workspaceId: string, tabId: string) => void;
  /** Reopen a recently-closed pane (index 0 = most recent). */
  reopenClosed: (workspaceId: string, index?: number) => void;
  /** Reopen a whole closed group (cascade subtree or workspace) by id. */
  reopenGroup: (groupId: string) => void;
  /** Reopen a pane from the orphaned ring (workspace was deleted). */
  restoreOrphaned: (paneId: string) => void;
  setActivepane: (workspaceId: string, tabId: string) => void;
  /** Select + focus the tab that owns `paneId` (resolves via a fresh snapshot
   *  when the pane isn't in local state yet). Used to jump to a just-spawned
   *  agent or orchestration pair. */
  selectTabByPane: (workspaceId: string, paneId: string) => Promise<void>;
  closePane: (paneId: string) => void;
  setFocusedPane: (paneId: string) => void;
  setPanePty: (paneId: string, ptyId: string) => void;
  addNotification: (notif: Omit<Notification, "id" | "timestamp" | "read">) => void;
  upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => void;
  markRead: (id: string) => void;
  markAllRead: () => void;
  deleteNotification: (id: string) => void;
  clearNotifications: () => void;
  dismissToast: (id: string) => void;
  setPaneTitle: (paneId: string, title: string, level?: TitleLevel) => void;
  setTabTitle: (workspaceId: string, tabId: string, title: string) => void;
  setAgentType: (paneId: string, agentType: string | null) => void;
  updatePaneStatus: (paneId: string, status: PaneStatus) => void;
  spawnClaudepane: (workspaceId: string, skipPermissions: boolean, cwd?: string) => void;
  spawnAgentpane: (
    workspaceId: string,
    agentId: string,
    skipPermissions: boolean,
    cwd?: string,
    model?: string,
  ) => Promise<SpawnedPane | null>;
  /** Paired creation (task_7e5f9354): create a plan plus an Orchestrator +
   *  Planner pane bound to it, in one call. */
  createOrchestrationPair: (input: OrchestrationPairInput) => Promise<OrchestrationPairResult>;
}

/** @deprecated Use MultiplexerActionsContextType — state reads go through useMuxState/useMuxSelector. */
export type MultiplexerContextType = MultiplexerActionsContextType;

export const MultiplexerActionsContext = createContext<MultiplexerActionsContextType | null>(null);
export const MuxStateStoreContext = createContext<MuxStateStore | null>(null);

/** Stable action callbacks — identity does not change when mux state updates. */
export function useMultiplexer(): MultiplexerActionsContextType {
  const ctx = useContext(MultiplexerActionsContext);
  if (!ctx) throw new Error("useMultiplexer must be used within MultiplexerProvider");
  return ctx;
}
