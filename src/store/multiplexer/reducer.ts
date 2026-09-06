import { TitleLevel } from "../../lib/types";
import type {
  MultiplexerState,
  Workspace,
  Tab,
  Notification,
  TabStatus,
  BootStage,
} from "../../lib/types";
import {
  highestTitleLevel,
  reconcileBackendWorkspaceTabs,
  type ExtendedState,
} from "./tree";
import type { RemoteTransportAvailability, RemoteTransportChannel } from "../../lib/remoteTabTransportState";

export type Action =
  | { type: "SET_STATE"; state: MultiplexerState }
  | { type: "ADD_WORKSPACE"; workspace: Workspace; tab: Tab }
  | { type: "SET_ACTIVE_WORKSPACE"; id: string }
  | { type: "CLOSE_WORKSPACE"; id: string }
  | { type: "RENAME_WORKSPACE"; id: string; name: string }
  | { type: "MOVE_TAB"; tabId: string; fromWorkspaceId: string; toWorkspaceId: string }
  | { type: "REORDER_TABS"; orderedTabIds: string[] }
  | { type: "REORDER_WORKSPACES"; orderedWorkspaceIds: string[] }
  | { type: "ADD_TAB"; workspaceId: string; tab: Tab }
  | { type: "CLOSE_TAB"; workspaceId: string; tabId: string; removedTabIds: string[] }
  | { type: "SET_ACTIVE_TAB"; workspaceId: string; tabId: string }
  | { type: "SET_FOCUSED_TAB"; tabId: string }
  | { type: "SET_TAB_PTY"; tabId: string; ptyId: string }
  | { type: "ADD_NOTIFICATION"; notification: Notification }
  | { type: "HYDRATE_NOTIFICATIONS"; notifications: Notification[] }
  | { type: "UPSERT_TOAST"; toast: Notification }
  | { type: "MARK_READ"; id: string }
  | { type: "ANSWER_NOTIFICATION"; id: string; answered: string; delivered: boolean }
  | { type: "MARK_ALL_READ" }
  | { type: "DELETE_NOTIFICATION"; id: string }
  | { type: "CLEAR_NOTIFICATIONS" }
  | { type: "DISMISS_TOAST"; id: string }
  | { type: "SET_TAB_TITLE"; tabId: string; title: string; level?: TitleLevel }
  | { type: "SET_AGENT_TYPE"; tabId: string; agentType: string | null }
  | { type: "UPDATE_TAB_STATUS"; tabId: string; status: TabStatus }
  | { type: "UPDATE_BOOT_STAGE"; tabId: string; stage: BootStage; reason?: string | null }
  | { type: "REMOVE_PENDING_CLOSE"; tabIds: string[] }
  | { type: "RESET_PEER_CONTEXT" }
  | { type: "RESET_GENERATION" }
  | { type: "SET_REMOTE_TABS"; tabs: Record<string, Tab> }
  | { type: "UPDATE_REMOTE_CHANNEL"; tabId: string; channel: RemoteTransportChannel; availability: RemoteTransportAvailability };

/**
 * Merge a fresh `tab_statuses` snapshot into the known map, then drop the
 * synthetic "Waiting" the backend derives from a tab's `pending_prompt` once
 * that prompt is gone and the fresh snapshot doesn't re-affirm any status for
 * the tab. Plain merging alone leaves "Waiting" stuck forever for a tab
 * with no attached PTY (nothing for `snapshot_with_status` to report once
 * `pending_prompt` clears, so the key is simply absent from every future
 * snapshot) — e.g. a hook-driven attention prompt on a tab that hasn't
 * opened a real terminal yet. Any OTHER status (real Idle/Working/Dead, or a
 * re-affirmed "Waiting" status) is left untouched since the fresh snapshot
 * explicitly carries it.
 */
function reconcileTabStatuses(
  prev: Record<string, TabStatus>,
  incoming: Record<string, TabStatus> | undefined,
  tabs: Record<string, Tab>,
): Record<string, TabStatus> {
  const merged = incoming ? { ...prev, ...incoming } : { ...prev };
  for (const id of Object.keys(merged)) {
    if (
      merged[id] === "Waiting" &&
      !tabs[id]?.pending_prompt &&
      !incoming?.[id]
    ) {
      delete merged[id];
    }
  }
  return merged;
}

/**
 * A non-sticky toast expires when its life animation ends, which only runs
 * while it is mounted. The rail mounts at most two, so anything pushed past
 * the render window never animates and never expires — a burst strands its
 * own overflow permanently behind a growing "+N". Bound the queue itself:
 * drop the oldest expendable entries. Sticky toasts and ones carrying actions
 * are answers the user still owes, so they are never dropped.
 */
const MAX_QUEUED_TOASTS = 12;

function capToasts(toasts: Notification[]): Notification[] {
  if (toasts.length <= MAX_QUEUED_TOASTS) return toasts;
  const expendable = (toast: Notification) => !toast.sticky && !toast.actions?.length;
  const excess = toasts.length - MAX_QUEUED_TOASTS;
  let dropped = 0;
  const kept = toasts.filter((toast) => {
    if (dropped >= excess || !expendable(toast)) return true;
    dropped += 1;
    return false;
  });
  return kept;
}

function attachRemoteTabs(
  workspaces: Workspace[],
  baseTabs: Record<string, Tab>,
  remoteTabs: Record<string, Tab>,
): { workspaces: Workspace[]; tabs: Record<string, Tab> } {
  const remoteIds = Object.keys(remoteTabs);
  const nextWorkspaces = workspaces.map((workspace, index) => ({
    ...workspace,
    tabs: [
      ...workspace.tabs.filter((id) => baseTabs[id]),
      ...remoteIds.filter((id) => {
        const target = remoteTabs[id]?.workspace_id;
        return target ? target === workspace.id : index === 0;
      }),
    ],
  }));
  return { workspaces: nextWorkspaces, tabs: { ...baseTabs, ...remoteTabs } };
}

function addPendingCloses(current: string[], tabIds: string[]): string[] {
  if (tabIds.length === 0) return current;
  const next = new Set(current);
  for (const id of tabIds) next.add(id);
  return next.size === current.length ? current : [...next];
}

export function reducer(state: ExtendedState, action: Action): ExtendedState {
  switch (action.type) {
    case "UPDATE_REMOTE_CHANNEL": {
      const tab = state.tabs[action.tabId];
      if (!tab?.remote_peer_id) return state;
      return {
        ...state,
        tabs: {
          ...state.tabs,
          [action.tabId]: {
            ...tab,
            remote_channels: { ...tab.remote_channels, [action.channel]: action.availability },
          },
        },
      };
    }
    case "SET_REMOTE_TABS": {
      const localTabs = Object.fromEntries(
        Object.entries(state.tabs).filter(([, tab]) => !tab.remote_peer_id),
      );
      const merged = attachRemoteTabs(state.workspaces, localTabs, action.tabs);
      return { ...state, workspaces: merged.workspaces, tabs: merged.tabs };
    }
    case "RESET_PEER_CONTEXT":
      return {
        ...state,
        workspaces: [],
        tabs: {},
        teams: [],
        active_workspace_id: null,
        focused_tab_id: null,
        tabStatuses: {},
        activeTabIds: {},
        generation: -1,
        pendingCloses: [],
      };

    case "SET_STATE": {
      const incomingGen = action.state.generation;
      if (typeof incomingGen === "number" && incomingGen <= state.generation) {
        return state;
      }
      // This snapshot is accepted. Apply optimistic-close tombstones now, so a
      // tombstone clears only against a snapshot the generation guard accepted.
      const stillPending: string[] = [];
      const maskedTabIds = new Set<string>();
      for (const tabId of state.pendingCloses) {
        const present =
          Boolean(action.state.tabs[tabId]) ||
          action.state.workspaces.some((workspace) => workspace.tabs.includes(tabId));
        if (present) {
          stillPending.push(tabId);
          maskedTabIds.add(tabId);
        }
      }
      const sourceTabs = maskedTabIds.size
        ? Object.fromEntries(
            Object.entries(action.state.tabs).filter(([id]) => !maskedTabIds.has(id)),
          )
        : action.state.tabs;
      const sourceWorkspaces = maskedTabIds.size
        ? action.state.workspaces.map((w) => ({
            ...w,
            tabs: w.tabs.filter((tabId) => !maskedTabIds.has(tabId)),
          }))
        : action.state.workspaces;
      // Backend is authoritative; one tab per tab means no split ratio.
      const mergedTabs = { ...sourceTabs };
      const mergedWorkspaces = reconcileBackendWorkspaceTabs(sourceWorkspaces, mergedTabs);
      for (const [id, backendTab] of Object.entries(mergedTabs)) {
        const existing = state.tabs[id];
        // Backend agent_type is authoritative — it reflects a provider switch
        // (codex→claude). Keep the last known value only while a restart
        const agentType = backendTab.agent_type ?? existing?.agent_type;
        const backendLevel = backendTab.title_level || TitleLevel.Default;
        const frontendLevel = existing?.title_level || TitleLevel.Default;
        const highestLevel = highestTitleLevel(backendLevel, frontendLevel);
        const keepFrontendTitle =
          existing &&
          existing.title_pending_local === true &&
          highestLevel === frontendLevel &&
          frontendLevel !== TitleLevel.Auto;
        const title = keepFrontendTitle ? existing.title : backendTab.title;
        // Backend has caught up once its title matches what we're keeping;
        // otherwise the local rename is still in flight.
        const title_pending_local = keepFrontendTitle && backendTab.title !== existing.title;
        mergedTabs[id] = {
          ...backendTab,
          title_level: highestLevel as TitleLevel,
          title,
          title_pending_local,
          ...(agentType ? { agent_type: agentType } : {}),
        };
      }
      // Remote-projected tabs live outside the local snapshot; re-attach them so an
      // authoritative local snapshot never drops their identity, selection, or focus.
      const remoteTabs = Object.fromEntries(
        Object.entries(state.tabs).filter(([, tab]) => Boolean(tab.remote_peer_id)),
      );
      const attached = attachRemoteTabs(mergedWorkspaces, mergedTabs, remoteTabs);
      const activeTabIds: Record<string, string> = {};
      for (const w of attached.workspaces) {
        const prevActive = state.activeTabIds[w.id];
        if (prevActive && attached.tabs[prevActive]?.remote_peer_id && w.tabs.includes(prevActive)) {
          activeTabIds[w.id] = prevActive;
        } else if (w.active_tab_id) {
          activeTabIds[w.id] = w.active_tab_id;
        }
      }
      // Preserve prior focus only while its tab still exists.
      const incomingFocus = action.state.focused_tab_id;
      const focused_tab_id =
        incomingFocus && attached.tabs[incomingFocus]
          ? incomingFocus
          : state.focused_tab_id && attached.tabs[state.focused_tab_id]
            ? state.focused_tab_id
            : null;
      return {
        ...state,
        workspaces: attached.workspaces,
        tabs: attached.tabs,
        teams: action.state.teams ?? [],
        focused_tab_id,
        active_workspace_id: action.state.active_workspace_id,
        activeTabIds,
        // Prefer the authoritative recently closed list when it is present.
        recently_closed: action.state.recently_closed ?? state.recently_closed,
        // Keep newer live status events when a snapshot omits them.
        tabStatuses: reconcileTabStatuses(
          state.tabStatuses,
          action.state.tab_statuses,
          mergedTabs,
        ),
        generation:
          typeof incomingGen === "number" ? incomingGen : state.generation,
        pendingCloses: stillPending,
      };
    }

    case "ADD_WORKSPACE": {
      return {
        ...state,
        workspaces: [...state.workspaces, action.workspace],
        tabs: { ...state.tabs, [action.tab.id]: action.tab },
        active_workspace_id: action.workspace.id,
      };
    }

    case "SET_ACTIVE_WORKSPACE":
      return { ...state, active_workspace_id: action.id };

    case "CLOSE_WORKSPACE": {
      const ws = state.workspaces.find((w) => w.id === action.id);
      const removedTabIds = ws ? [...ws.tabs] : [];
      const newTabs = { ...state.tabs };
      const newStatuses = { ...state.tabStatuses };
      for (const pid of removedTabIds) {
        delete newTabs[pid];
        delete newStatuses[pid];
      }
      const remaining = state.workspaces.filter((w) => w.id !== action.id);
      const newActiveTabIds = { ...state.activeTabIds };
      delete newActiveTabIds[action.id];
      // keep focused_tab_id untouched; close_workspace emits fallback focus separately.
      return {
        ...state,
        workspaces: remaining,
        tabs: newTabs,
        tabStatuses: newStatuses,
        activeTabIds: newActiveTabIds,
        pendingCloses: addPendingCloses(state.pendingCloses, removedTabIds),
        active_workspace_id:
          state.active_workspace_id === action.id
            ? (remaining[0]?.id ?? null)
            : state.active_workspace_id,
      };
    }

    case "RENAME_WORKSPACE": {
      return {
        ...state,
        workspaces: state.workspaces.map((ws) =>
          ws.id === action.id ? { ...ws, name: action.name } : ws,
        ),
      };
    }

    case "MOVE_TAB": {
      const fromWs = state.workspaces.find((w) => w.id === action.fromWorkspaceId);
      const tabId = fromWs?.tabs.find((id) => id === action.tabId);
      if (!fromWs || !tabId || fromWs.tabs.length <= 1) return state;

      const newActiveTabIds = { ...state.activeTabIds };
      const fromActiveTabId = newActiveTabIds[action.fromWorkspaceId];
      if (fromActiveTabId === action.tabId) {
        const remaining = fromWs.tabs.filter((id) => id !== action.tabId);
        const fallback = remaining[remaining.length - 1];
        if (fallback) {
          newActiveTabIds[action.fromWorkspaceId] = fallback;
        } else {
          delete newActiveTabIds[action.fromWorkspaceId];
        }
      }
      newActiveTabIds[action.toWorkspaceId] = action.tabId;

      return {
        ...state,
        workspaces: state.workspaces.map((ws) => {
          if (ws.id === action.fromWorkspaceId) {
            const newTabs = ws.tabs.filter((id) => id !== action.tabId);
            return {
              ...ws,
              tabs: newTabs,
              active_tab_id: ws.active_tab_id === action.tabId
                ? newTabs[newTabs.length - 1] ?? null
                : ws.active_tab_id,
            };
          }
          if (ws.id === action.toWorkspaceId) {
            return {
              ...ws,
              tabs: [...ws.tabs, tabId],
              active_tab_id: action.tabId,
            };
          }
          return ws;
        }),
        activeTabIds: newActiveTabIds,
        active_workspace_id: action.toWorkspaceId,
      };
    }

    case "REORDER_TABS": {
      const orderById = new Map(action.orderedTabIds.map((id, i) => [id, i]));
      return {
        ...state,
        workspaces: state.workspaces.map((ws) => {
          if (!ws.tabs.some((tabId) => orderById.has(tabId))) return ws;
          return {
            ...ws,
            tabs: [...ws.tabs].sort(
              (a, b) => (orderById.get(a) ?? Number.MAX_SAFE_INTEGER)
                - (orderById.get(b) ?? Number.MAX_SAFE_INTEGER),
            ),
          };
        }),
      };
    }

    case "REORDER_WORKSPACES": {
      const orderById = new Map(action.orderedWorkspaceIds.map((id, i) => [id, i]));
      return {
        ...state,
        workspaces: state.workspaces.map((ws) =>
          orderById.has(ws.id) ? { ...ws, sort_order: orderById.get(ws.id)! } : ws,
        ),
      };
    }

    case "ADD_TAB": {
      // Caller decides focus on tab creation; don't auto-focus here.
      return {
        ...state,
        workspaces: state.workspaces.map((ws) =>
          ws.id === action.workspaceId
            ? { ...ws, tabs: [...ws.tabs, action.tab.id], active_tab_id: action.tab.id }
            : ws,
        ),
        tabs: { ...state.tabs, [action.tab.id]: action.tab },
        activeTabIds: { ...state.activeTabIds, [action.workspaceId]: action.tab.id },
      };
    }

    case "CLOSE_TAB": {
      const newTabs = { ...state.tabs };
      const newStatuses = { ...state.tabStatuses };
      const removedSet = new Set(action.removedTabIds);
      for (const pid of action.removedTabIds) {
        delete newTabs[pid];
        delete newStatuses[pid];
      }
      const newActiveTabIds = { ...state.activeTabIds };
      // focused_tab_id intentionally untouched — see CLOSE_WORKSPACE comment.
      return {
        ...state,
        workspaces: state.workspaces.map((ws) => {
          if (ws.id !== action.workspaceId) return ws;
          const removedTabIds = new Set(
            ws.tabs.filter((id) => id === action.tabId || removedSet.has(id)),
          );
          const remaining = ws.tabs.filter((id) => !removedTabIds.has(id));
          const currentActive = newActiveTabIds[action.workspaceId];
          if (currentActive && removedTabIds.has(currentActive)) {
            const fallback = remaining[remaining.length - 1];
            if (fallback) {
              newActiveTabIds[action.workspaceId] = fallback;
            } else {
              delete newActiveTabIds[action.workspaceId];
            }
          }
          return {
            ...ws,
            tabs: remaining,
            active_tab_id:
              ws.active_tab_id && removedTabIds.has(ws.active_tab_id)
                ? (remaining[remaining.length - 1] ?? null)
                : ws.active_tab_id,
          };
        }),
        tabs: newTabs,
        tabStatuses: newStatuses,
        activeTabIds: newActiveTabIds,
        pendingCloses: addPendingCloses(state.pendingCloses, action.removedTabIds),
      };
    }

    case "SET_ACTIVE_TAB":
      return {
        ...state,
        activeTabIds: { ...state.activeTabIds, [action.workspaceId]: action.tabId },
      };

    case "REMOVE_PENDING_CLOSE": {
      if (action.tabIds.length === 0) return state;
      const remove = new Set(action.tabIds);
      const next = state.pendingCloses.filter((id) => !remove.has(id));
      return next.length === state.pendingCloses.length
        ? state
        : { ...state, pendingCloses: next };
    }

    case "SET_FOCUSED_TAB":
      // Ignore focus updates for unknown tabs while state merge is in flight.
      if (action.tabId && !state.tabs[action.tabId]) return state;
      return { ...state, focused_tab_id: action.tabId };

    case "SET_TAB_PTY": {
      const tab = state.tabs[action.tabId];
      if (!tab) return state;
      return {
        ...state,
        tabs: {
          ...state.tabs,
          [action.tabId]: { ...tab, pty_id: action.ptyId },
        },
      };
    }

    case "SET_TAB_TITLE": {
      const tab = state.tabs[action.tabId];
      if (!tab) return state;
      const levels = [TitleLevel.Default, TitleLevel.Auto, TitleLevel.Agent, TitleLevel.User];
      const newLevel = action.level || TitleLevel.Auto;
      const curLevel = tab.title_level || TitleLevel.Default;
      if (levels.indexOf(newLevel) < levels.indexOf(curLevel)) return state;
      return {
        ...state,
        tabs: {
          ...state.tabs,
          [action.tabId]: {
            ...tab,
            title: action.title,
            title_level: newLevel,
            title_pending_local: true,
          },
        },
      };
    }

    // A coalesced notification is re-emitted under its existing id; replacing it
    // keeps one row instead of stacking a second copy of the same event.
    case "ADD_NOTIFICATION": {
      const replaceById = <T extends { id: string }>(list: T[], next: T): T[] => {
        const index = list.findIndex((entry) => entry.id === next.id);
        if (index === -1) return [...list, next];
        const updated = [...list];
        updated[index] = next;
        return updated;
      };
      return {
        ...state,
        notifications: replaceById(state.notifications, action.notification),
        toasts: capToasts(replaceById(state.toasts, action.notification)),
      };
    }

    case "UPSERT_TOAST": {
      const existingIndex = action.toast.key
        ? state.toasts.findIndex((toast) => toast.key === action.toast.key)
        : state.toasts.findIndex((toast) => toast.id === action.toast.id);
      if (existingIndex === -1) {
        return { ...state, toasts: capToasts([...state.toasts, action.toast]) };
      }
      const toasts = [...state.toasts];
      toasts[existingIndex] = action.toast;
      return { ...state, toasts };
    }

    // The one-answer invariant lives in the store; this only reflects what the
    // daemon recorded, so a second tap can never overwrite it here either.
    case "ANSWER_NOTIFICATION": {
      // The label is written once; whether it reached the tab may still change,
      // because a standing answer the envelope path refused can be carried later.
      const withAnswer = (entry: Notification): Notification =>
        entry.id !== action.id || !entry.answer
          ? entry
          : {
              ...entry,
              answer: {
                ...entry.answer,
                answered: entry.answer.answered ?? action.answered,
                delivered: action.delivered,
              },
            };
      return {
        ...state,
        notifications: state.notifications.map(withAnswer),
        toasts: state.toasts.map(withAnswer),
      };
    }

    case "MARK_READ":
      return {
        ...state,
        notifications: state.notifications.map((n) =>
          n.id === action.id ? { ...n, read: true } : n,
        ),
      };

    case "MARK_ALL_READ":
      return {
        ...state,
        notifications: state.notifications.map((n) => ({ ...n, read: true })),
      };

    case "HYDRATE_NOTIFICATIONS": {
      const notifications = new Map(action.notifications.map((notification) => [notification.id, notification]));
      for (const notification of state.notifications) notifications.set(notification.id, notification);
      return { ...state, notifications: [...notifications.values()] };
    }

    case "DELETE_NOTIFICATION":
      return {
        ...state,
        notifications: state.notifications.filter((n) => n.id !== action.id),
        toasts: state.toasts.filter((n) => n.id !== action.id),
      };

    case "CLEAR_NOTIFICATIONS":
      return {
        ...state,
        notifications: [],
      };

    case "DISMISS_TOAST":
      return {
        ...state,
        toasts: state.toasts.filter((t) => t.id !== action.id),
      };

    case "SET_AGENT_TYPE": {
      const agentTab = state.tabs[action.tabId];
      if (!agentTab) return state;
      return {
        ...state,
        tabs: {
          ...state.tabs,
          [action.tabId]: { ...agentTab, agent_type: action.agentType },
        },
      };
    }

    case "UPDATE_TAB_STATUS":
      return {
        ...state,
        tabStatuses: {
          ...state.tabStatuses,
          [action.tabId]: action.status,
        },
      };

    case "UPDATE_BOOT_STAGE": {
      const tab = state.tabs[action.tabId];
      if (!tab) return state;
      return {
        ...state,
        tabs: {
          ...state.tabs,
          [action.tabId]: {
            ...tab,
            boot_stage: action.stage,
            boot_failure_reason: action.stage === "spawn_failed" ? (action.reason ?? null) : null,
          },
        },
      };
    }

    case "RESET_GENERATION":
      return state.generation === 0 ? state : { ...state, generation: 0 };

    default:
      return state;
  }
}
