import { useSyncExternalStore } from "react";
import { invoke, listen } from "./bridge";

export type PluginTabMenuAction = "receiverTarget";

/** One manifest-declared item; the host owns what its action does. */
export interface PluginTabMenuItem {
  pluginId: string;
  itemId: string;
  label: string;
  action: PluginTabMenuAction;
}

export interface PluginTabMenuState {
  items: PluginTabMenuItem[];
  /** Receiver plugin id → the one tab id bound to it. */
  receiverTargets: Record<string, string>;
}

export interface PluginTabMenuEntry extends PluginTabMenuItem {
  checked: boolean;
}

type Runner = (item: PluginTabMenuItem, tabId: string) => Promise<PluginTabMenuState>;

const EMPTY: PluginTabMenuState = { items: [], receiverTargets: {} };
const listeners = new Set<() => void>();
let state: PluginTabMenuState = EMPTY;
let subscribed = false;
let inFlight = false;
let stale = false;
let runner: Runner = (item, tabId) =>
  invoke<PluginTabMenuState>("plugin_tab_menu_action", {
    pluginId: item.pluginId,
    itemId: item.itemId,
    tabId,
  });

function publish(next: PluginTabMenuState): void {
  state = next;
  for (const listener of listeners) listener();
}

export function pluginTabMenuEntries(menu: PluginTabMenuState, tabId: string): PluginTabMenuEntry[] {
  return menu.items.map((item) => ({
    ...item,
    checked: item.action === "receiverTarget" && menu.receiverTargets[item.pluginId] === tabId,
  }));
}

export async function refreshPluginTabMenu(): Promise<void> {
  if (inFlight) {
    stale = true;
    return;
  }
  inFlight = true;
  try {
    do {
      stale = false;
      publish(await invoke<PluginTabMenuState>("plugin_tab_menu"));
    } while (stale);
  } catch (error) {
    console.warn("Plugin tab menu refresh failed:", error);
  } finally {
    inFlight = false;
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  if (!subscribed) {
    subscribed = true;
    void refreshPluginTabMenu();
    listen("contributions-changed", () => {
      void refreshPluginTabMenu();
    }).catch(() => {
      subscribed = false;
    });
  }
  return () => {
    listeners.delete(listener);
  };
}

export function usePluginTabMenu(): PluginTabMenuState {
  return useSyncExternalStore(subscribe, () => state, () => state);
}

export async function runPluginTabMenuItem(item: PluginTabMenuItem, tabId: string): Promise<void> {
  publish(await runner(item, tabId));
}

export function __setPluginTabMenuForTest(next: PluginTabMenuState, testRunner?: Runner): void {
  subscribed = true;
  if (testRunner) runner = testRunner;
  publish(next);
}
