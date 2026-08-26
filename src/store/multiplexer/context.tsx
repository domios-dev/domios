import { createContext, useContext } from "react";
import type {
  Notification,
  TabStatus,
  OrchestrationPairInput,
  OrchestrationPairResult,
} from "../../lib/types";
import type { RemotePeerTabs } from "../../lib/remoteTabPortal";
import type { MuxStateStore } from "./selectorStore";

/** Ids returned by a successful spawn — enough to resolve + select the tab. */
export interface SpawnedTab {
  workspaceId: string;
  tabId: string;
}

export interface MultiplexerActionsContextType {
  createWorkspace: (name: string, cwd?: string, opts?: { activate?: boolean }) => Promise<{ id: string; name: string } | undefined>;
  setActiveWorkspace: (id: string) => void;
  closeWorkspace: (id: string) => void;
  renameWorkspace: (id: string, name: string) => void;
  moveTab: (tabId: string, fromWorkspaceId: string, toWorkspaceId: string) => void;
  /** Reorder a tab group within its hierarchy level. `parentTabId` set →
   *  reorder children of that parent; otherwise reorder a workspace's top-level
   *  tabs. Within-group only — the order list never crosses groups. */
  reorderTabs: (args: { workspaceId?: string; parentTabId?: string; orderedTabIds: string[] }) => void;
  /** Reorder the workspace list to the given order. */
  reorderWorkspaces: (orderedWorkspaceIds: string[]) => void;
  createTab: (workspaceId: string, cwd?: string) => Promise<{ id: string } | void>;
  attachRemoteTab?: (workspaceId: string, peer: RemotePeerTabs, tab: RemotePeerTabs["tabs"][number]) => void;
  closeTab: (workspaceId: string, tabId: string) => void;
  /** Reopen a recently-closed tab (index 0 = most recent). */
  reopenClosed: (workspaceId: string, index?: number) => void;
  /** Reopen a whole closed group (cascade subtree or workspace) by id. */
  reopenGroup: (groupId: string) => void;
  /** Reopen a tab from the orphaned ring (workspace was deleted). */
  restoreOrphaned: (tabId: string) => void;
  setActiveTab: (workspaceId: string, tabId: string) => void;
  /** Select + focus the tab that owns `tabId` (resolves via a fresh snapshot
   *  when the tab isn't in local state yet). Used to jump to a just-spawned
   *  agent or orchestration pair. */
  selectTabByTab: (workspaceId: string, tabId: string) => Promise<void>;
  setFocusedTab: (tabId: string) => void;
  setTabPty: (tabId: string, ptyId: string) => void;
  addNotification: (notif: Omit<Notification, "id" | "timestamp" | "read">) => void;
  upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => void;
  markRead: (id: string) => void;
  markAllRead: () => void;
  deleteNotification: (id: string) => void;
  clearNotifications: () => void;
  answerNotification: (id: string, label: string) => void;
  dismissToast: (id: string) => void;
  setTabTitle: (workspaceId: string, tabId: string, title: string) => void;
  setAgentType: (tabId: string, agentType: string | null) => void;
  updateTabStatus: (tabId: string, status: TabStatus) => void;
  spawnClaudeTab: (workspaceId: string, cwd?: string) => void;
  spawnAgentTab: (
    workspaceId: string,
    agentId: string,
    cwd?: string,
    model?: string,
  ) => Promise<SpawnedTab | null>;
  /** Paired creation (task_7e5f9354): create a plan plus an Orchestrator +
   *  Planner tab bound to it, in one call. */
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
