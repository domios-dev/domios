import { useSyncExternalStore } from "react";
import { invoke, listen, type UnlistenFn } from "./bridge";
import { hostBridgeAvailable } from "./hostBridge";

export type PluginTabMenuAction = "receiverTarget";

/** One manifest-declared item; the host owns what its action does. */
export interface PluginTabMenuItem {
  pluginId: string;
  itemId: string;
  label: string;
  action: PluginTabMenuAction;
  iconSvg?: string | null;
  iconColor?: string | null;
}

export interface PluginTabMenuState {
  items: PluginTabMenuItem[];
  /** Receiver plugin id → the one tab id bound to it. */
  receiverTargets: Record<string, string>;
}

export interface PluginTabMenuEntry extends PluginTabMenuItem {
  checked: boolean;
}

export interface PluginTabMenuHost {
  available(): boolean;
  load(): Promise<PluginTabMenuState>;
  run(item: PluginTabMenuItem, tabId: string): Promise<PluginTabMenuState>;
  watch(onChange: () => void): Promise<UnlistenFn>;
}

const bridgeHost: PluginTabMenuHost = {
  available: hostBridgeAvailable,
  load: () => invoke<PluginTabMenuState>("plugin_tab_menu"),
  run: (item, tabId) =>
    invoke<PluginTabMenuState>("plugin_tab_menu_action", {
      pluginId: item.pluginId,
      itemId: item.itemId,
      tabId,
    }),
  watch: (onChange) => listen("contributions-changed", () => onChange()),
};

const EMPTY: PluginTabMenuState = { items: [], receiverTargets: {} };
const listeners = new Set<() => void>();
let host: PluginTabMenuHost = bridgeHost;
let state: PluginTabMenuState = EMPTY;
let syncing = false;
let syncGeneration = 0;
let release: UnlistenFn | null = null;
let inFlight = false;
let stale = false;

function publish(next: PluginTabMenuState): void {
  // A host that answers with the wrong shape must not take the sidebar down.
  state = {
    items: Array.isArray(next?.items) ? next.items : [],
    receiverTargets: next?.receiverTargets && typeof next.receiverTargets === "object" ? next.receiverTargets : {},
  };
  for (const listener of listeners) listener();
}

export function pluginTabMenuEntries(menu: PluginTabMenuState, tabId: string): PluginTabMenuEntry[] {
  return menu.items.map((item) => ({
    ...item,
    checked: item.action === "receiverTarget" && menu.receiverTargets[item.pluginId] === tabId,
  }));
}

export async function refreshPluginTabMenu(): Promise<void> {
  if (!host.available()) return;
  if (inFlight) {
    stale = true;
    return;
  }
  inFlight = true;
  try {
    do {
      stale = false;
      publish(await host.load());
    } while (stale);
  } catch (error) {
    console.warn("Plugin tab menu refresh failed:", error);
  } finally {
    inFlight = false;
  }
}

function startSync(): void {
  if (syncing || !host.available()) return;
  syncing = true;
  const generation = ++syncGeneration;
  void refreshPluginTabMenu();
  host
    .watch(() => {
      void refreshPluginTabMenu();
    })
    .then((unlisten) => {
      // A last unsubscribe that won the race already stopped this sync; drop its listener at once.
      if (syncing && generation === syncGeneration) release = unlisten;
      else unlisten();
    })
    .catch((error) => {
      console.warn("Plugin tab menu listener failed:", error);
      if (generation === syncGeneration) syncing = false;
    });
}

function stopSync(): void {
  syncing = false;
  syncGeneration += 1;
  const unlisten = release;
  release = null;
  unlisten?.();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  startSync();
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) stopSync();
  };
}

export function usePluginTabMenu(): PluginTabMenuState {
  return useSyncExternalStore(subscribe, () => state, () => state);
}

export async function runPluginTabMenuItem(item: PluginTabMenuItem, tabId: string): Promise<void> {
  publish(await host.run(item, tabId));
}

export function __setPluginTabMenuForTest(next: PluginTabMenuState, testHost: PluginTabMenuHost): void {
  host = testHost;
  publish(next);
}

export function __pluginTabMenuSyncForTest(): { subscribers: number; listening: boolean } {
  return { subscribers: listeners.size, listening: syncing || release !== null };
}
