// Shared cache behind PluginBubbles (codeterm#336 part C): one entry per
// (pluginId, cwd), computed only on a hidden->visible transition or an invalidation — no interval.
import { invoke, listen } from "./bridge";
import type { Tone } from "../components/Extensions/ViewNodeRenderer";

export interface BubbleData {
  label: string;
  tone?: Tone;
  icon?: string | null;
  tooltip?: string | null;
  action?: string | null;
}

export interface PluginBubbleSubscription {
  setActive: (active: boolean) => void;
  unsubscribe: () => void;
}

type Fetcher = (
  pluginId: string,
  cwd: string | null,
  tabId: string,
  identityChanged: boolean,
) => Promise<BubbleData | null>;

function defaultFetcher(
  pluginId: string,
  cwd: string | null,
  tabId: string,
  identityChanged: boolean,
): Promise<BubbleData | null> {
  return invoke<BubbleData | null>("plugin_bubble", {
    pluginId,
    ctx: { cwd, tabId, forceIdentityRefresh: identityChanged },
  });
}

let fetcher: Fetcher = defaultFetcher;

export function __setPluginBubbleFetcherForTest(fn: Fetcher | null): void {
  fetcher = fn ?? defaultFetcher;
}

interface Subscriber {
  tabId: string;
  active: boolean;
  onChange: (value: BubbleData | null) => void;
}

interface Entry {
  pluginId: string;
  cwd: string | null;
  value: BubbleData | null | undefined;
  inFlight: boolean;
  pendingRefresh: boolean;
  pendingIdentityChanged: boolean;
  subscribers: Map<string, Subscriber>;
}

const cache = new Map<string, Map<string | null, Entry>>();

function getOrCreateEntry(pluginId: string, cwd: string | null): Entry {
  let byCwd = cache.get(pluginId);
  if (!byCwd) {
    byCwd = new Map();
    cache.set(pluginId, byCwd);
  }
  let entry = byCwd.get(cwd);
  if (!entry) {
    entry = {
      pluginId,
      cwd,
      value: undefined,
      inFlight: false,
      pendingRefresh: false,
      pendingIdentityChanged: false,
      subscribers: new Map(),
    };
    byCwd.set(cwd, entry);
  }
  return entry;
}

function deleteEntryIfEmpty(entry: Entry): void {
  if (entry.subscribers.size > 0) return;
  const byCwd = cache.get(entry.pluginId);
  byCwd?.delete(entry.cwd);
  if (byCwd && byCwd.size === 0) cache.delete(entry.pluginId);
}

function activeCount(entry: Entry): number {
  let n = 0;
  for (const s of entry.subscribers.values()) if (s.active) n++;
  return n;
}

function notify(entry: Entry): void {
  for (const s of entry.subscribers.values()) s.onChange(entry.value ?? null);
}

function pickTabId(entry: Entry): string {
  for (const s of entry.subscribers.values()) if (s.active) return s.tabId;
  for (const s of entry.subscribers.values()) return s.tabId;
  return "";
}

function computeNow(entry: Entry, identityChanged: boolean): void {
  if (entry.inFlight) {
    entry.pendingRefresh = true;
    entry.pendingIdentityChanged = entry.pendingIdentityChanged || identityChanged;
    return;
  }
  entry.inFlight = true;
  fetcher(entry.pluginId, entry.cwd, pickTabId(entry), identityChanged)
    .then((value) => {
      entry.value = value;
      notify(entry);
    })
    .catch(() => {})
    .finally(() => {
      entry.inFlight = false;
      const shouldRefresh = entry.pendingRefresh && activeCount(entry) > 0;
      const nextIdentityChanged = entry.pendingIdentityChanged;
      entry.pendingRefresh = false;
      entry.pendingIdentityChanged = false;
      if (shouldRefresh) computeNow(entry, nextIdentityChanged);
    });
}

/** Subscribe once per (plugin, repo, tab) identity; call the returned
 * `setActive` on visibility changes instead of re-subscribing. */
export function subscribePluginBubble(
  pluginId: string,
  cwd: string | null,
  tabId: string,
  active: boolean,
  onChange: (value: BubbleData | null) => void,
): PluginBubbleSubscription {
  ensureTabStatusListener();
  const entry = getOrCreateEntry(pluginId, cwd);
  const subscriberId = `${tabId}:${Math.random().toString(36).slice(2)}`;
  entry.subscribers.set(subscriberId, { tabId, active: false, onChange });

  if (entry.value !== undefined) onChange(entry.value);

  const setActive = (nextActive: boolean): void => {
    const sub = entry.subscribers.get(subscriberId);
    if (!sub) return;
    const wasActive = sub.active;
    sub.active = nextActive;
    if (nextActive && !wasActive) computeNow(entry, false);
  };
  setActive(active);

  return {
    setActive,
    unsubscribe: () => {
      entry.subscribers.delete(subscriberId);
      deleteEntryIfEmpty(entry);
    },
  };
}

/** Explicit invalidation, coalesced with any in-flight refresh; a no-op
 * unless a visible tab is watching. `identityChanged` forces root/branch too. */
export function invalidatePluginBubble(
  pluginId: string,
  cwd: string | null | undefined,
  identityChanged = false,
): void {
  if (cwd === undefined) return;
  const entry = cache.get(pluginId)?.get(cwd);
  if (!entry || activeCount(entry) === 0) return;
  computeNow(entry, identityChanged);
}

// tab-status-changed can't say whether it involved a branch change, so it
// conservatively forces an identity refresh — never an indefinite stale cache.
function invalidateForTab(tabId: string): void {
  for (const byCwd of cache.values()) {
    for (const entry of byCwd.values()) {
      const isWatching = [...entry.subscribers.values()].some((s) => s.tabId === tabId && s.active);
      if (isWatching) computeNow(entry, true);
    }
  }
}

let tabStatusUnlisten: (() => void) | null = null;
let tabStatusArmed = false;

// tab-status-changed is the daemon's own ownership-boundary signal for
// "this tab did something" — the external-change event source, no watcher.
function ensureTabStatusListener(): void {
  if (tabStatusArmed) return;
  tabStatusArmed = true;
  void listen<{ tab_id: string }>("tab-status-changed", (event) => {
    invalidateForTab(event.payload.tab_id);
  }).then((unlisten) => {
    tabStatusUnlisten = unlisten;
  });
}

export function __resetPluginBubbleCacheForTest(): void {
  cache.clear();
  fetcher = defaultFetcher;
  tabStatusUnlisten?.();
  tabStatusUnlisten = null;
  tabStatusArmed = false;
}
