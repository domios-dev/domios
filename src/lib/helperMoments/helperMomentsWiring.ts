import { useEffect, useSyncExternalStore } from "react";
import type { CommandLifecycle } from "@codeterm/chat-core";
import { invoke, listen } from "../bridge";
import { buildCatalog } from "../../components/Extensions/pluginCatalog";
import type { AutomationGraph } from "../../components/Automation/types";
import { getPluginSnapshotState, subscribePluginSnapshotStore, type PluginSnapshotStoreState } from "../pluginSnapshotStore";
import type { Tab, TabStatus } from "../types";
import type { HelperPluginFact } from "./helperBlocks";
import { createHelperMomentsStore, type ActiveHelperSuggestion, type HelperMomentsStore } from "./helperMomentsStore";
import type { HelperTabKind } from "./momentRegistry";

const STORAGE_KEY = "codeterm.helperMoments.v1";

/** Agent tabs can wait on the user; only a person's own plain shell counts as manual commands. */
export function helperTabKind(tab: Tab | undefined): HelperTabKind {
  if (!tab) return "other";
  if (tab.agent_type) return "agent";
  const ownShell = !tab.managed && !tab.team_id && !tab.watcher && (tab.purpose ?? "shell") === "shell";
  return ownShell ? "manual-shell" : "other";
}

export function pluginFacts(state: Pick<PluginSnapshotStoreState, "hasLoaded" | "snapshot">): HelperPluginFact[] | null {
  if (!state.hasLoaded) return null;
  return buildCatalog(state.snapshot).map((entry) => ({ id: entry.id, name: entry.displayName, installed: entry.installed }));
}

let tabs: Readonly<Record<string, Tab>> = {};

export const helperMoments: HelperMomentsStore = createHelperMomentsStore({
  load: () => JSON.parse(window.localStorage.getItem(STORAGE_KEY) ?? "null"),
  save: (memory) => {
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(memory));
    } catch {
      return;
    }
  },
  tabKind: (tabId) => helperTabKind(tabs[tabId]),
  blueprintsInUse: async (tabId) => {
    const workspaceId = tabs[tabId]?.workspace_id;
    if (!workspaceId) return null;
    const graph = await invoke<AutomationGraph>("automation_graph_workspace_snapshot", { workspaceId });
    return (graph?.blueprints?.length ?? 0) > 0;
  },
});

/** Feeds the existing focus, tab-status, shell-command and plugin-catalog events into the helper; no polling. */
export function startHelperMoments(store: HelperMomentsStore = helperMoments): () => void {
  let stopped = false;
  const unlisteners: Array<() => void> = [];
  const keep = (unlisten: () => void) => (stopped ? unlisten() : unlisteners.push(unlisten));
  const focus = (focused: boolean) => () => void store.observe({ kind: "window-focus", focused, at: Date.now() });
  const onFocus = focus(true);
  const onBlur = focus(false);
  window.addEventListener("focus", onFocus);
  window.addEventListener("blur", onBlur);
  const syncPlugins = () => store.setFacts({ plugins: pluginFacts(getPluginSnapshotState()) });
  syncPlugins();
  keep(subscribePluginSnapshotStore(syncPlugins));
  void listen<{ tab_id: string; status: TabStatus }>("tab-status-changed", ({ payload }) => {
    if (payload?.tab_id) void store.observe({ kind: "tab-status", tabId: payload.tab_id, status: payload.status, at: Date.now() });
  }).then(keep, () => undefined);
  void listen<{ tab_id: string; event: CommandLifecycle }>("shell-command-changed", ({ payload }) => {
    const event = payload?.event;
    if (payload?.tab_id && event?.kind === "finished" && event.commandLine) {
      void store.observe({ kind: "shell-command", tabId: payload.tab_id, commandLine: event.commandLine, at: Date.now() });
    }
  }).then(keep, () => undefined);
  return () => {
    stopped = true;
    window.removeEventListener("focus", onFocus);
    window.removeEventListener("blur", onBlur);
    for (const unlisten of unlisteners.splice(0)) unlisten();
  };
}

/** App-level host: keeps tab classification and the Telegram fact current, and runs the listeners once. */
export function useHelperMomentsHost(currentTabs: Readonly<Record<string, Tab>>, telegramConnected: boolean | null) {
  tabs = currentTabs;
  useEffect(() => startHelperMoments(), []);
  useEffect(() => helperMoments.setFacts({ telegramConnected }), [telegramConnected]);
}

export function useHelperSuggestion(store: HelperMomentsStore = helperMoments): ActiveHelperSuggestion | null {
  return useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot).active;
}
