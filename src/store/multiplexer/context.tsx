import { createContext, useContext } from "react";
import { TitleLevel } from "../../lib/types";
import type { Notification, PaneStatus } from "../../lib/types";
import type { ExtendedState } from "./tree";

export interface MultiplexerContextType {
  state: ExtendedState;
  createWorkspace: (name: string, cwd?: string) => void;
  setActiveWorkspace: (id: string) => void;
  closeWorkspace: (id: string) => void;
  renameWorkspace: (id: string, name: string) => void;
  moveTab: (tabId: string, fromWorkspaceId: string, toWorkspaceId: string) => void;
  createTab: (workspaceId: string, cwd?: string) => void;
  closeTab: (workspaceId: string, tabId: string) => void;
  /** Reopen a recently-closed pane (index 0 = most recent). */
  reopenClosed: (workspaceId: string, index?: number) => void;
  /** Reopen a whole closed group (cascade subtree or workspace) by id. */
  reopenGroup: (groupId: string) => void;
  /** Reopen a pane from the orphaned ring (workspace was deleted). */
  restoreOrphaned: (paneId: string) => void;
  setActiveTab: (workspaceId: string, tabId: string) => void;
  closePane: (paneId: string) => void;
  setFocusedPane: (paneId: string) => void;
  setPanePty: (paneId: string, ptyId: string) => void;
  addNotification: (notif: Omit<Notification, "id" | "timestamp" | "read">) => void;
  upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => void;
  markRead: (id: string) => void;
  markAllRead: () => void;
  clearNotifications: () => void;
  dismissToast: (id: string) => void;
  setPaneTitle: (paneId: string, title: string, level?: TitleLevel) => void;
  setTabTitle: (workspaceId: string, tabId: string, title: string) => void;
  setAgentType: (paneId: string, agentType: string | null) => void;
  updatePaneStatus: (paneId: string, status: PaneStatus) => void;
  spawnClaudeTab: (workspaceId: string, skipPermissions: boolean, cwd?: string) => void;
  spawnAgentTab: (workspaceId: string, agentId: string, skipPermissions: boolean, cwd?: string) => void;
}

export const MultiplexerContext = createContext<MultiplexerContextType | null>(null);

export function useMultiplexer(): MultiplexerContextType {
  const ctx = useContext(MultiplexerContext);
  if (!ctx) throw new Error("useMultiplexer must be used within MultiplexerProvider");
  return ctx;
}
