// Shared cache behind PluginBubbles (codeterm#336 part C): one entry per
// (pluginId, cwd), computed only on a hidden->visible transition or an invalidation — no interval.
import { invoke } from "./bridge";
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

type Fetcher = (pluginId: string, cwd: string | null, tabId: string) => Promise<BubbleData | null>;

function defaultFetcher(pluginId: string, cwd: string | null, tabId: string): Promise<BubbleData | null> {
  return invoke<BubbleData | null>("plugin_bubble", { pluginId, ctx: { cwd, tabId } });
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
    entry = { pluginId, cwd, value: undefined, inFlight: false, pendingRefresh: false, subscribers: new Map() };
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

function computeNow(entry: Entry): void {
  if (entry.inFlight) {
    entry.pendingRefresh = true;
    return;
  }
  entry.inFlight = true;
  fetcher(entry.pluginId, entry.cwd, pickTabId(entry))
    .then((value) => {
      entry.value = value;
      notify(entry);
    })
    .catch(() => {})
    .finally(() => {
      entry.inFlight = false;
      const shouldRefresh = entry.pendingRefresh && activeCount(entry) > 0;
      entry.pendingRefresh = false;
      if (shouldRefresh) computeNow(entry);
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
  const entry = getOrCreateEntry(pluginId, cwd);
  const subscriberId = `${tabId}:${Math.random().toString(36).slice(2)}`;
  entry.subscribers.set(subscriberId, { tabId, active: false, onChange });

  if (entry.value !== undefined) onChange(entry.value);

  const setActive = (nextActive: boolean): void => {
    const sub = entry.subscribers.get(subscriberId);
    if (!sub) return;
    const wasActive = sub.active;
    sub.active = nextActive;
    if (nextActive && !wasActive) computeNow(entry);
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
 * unless a visible tab is watching this repo identity. */
export function invalidatePluginBubble(pluginId: string, cwd: string | null | undefined): void {
  if (cwd === undefined) return;
  const entry = cache.get(pluginId)?.get(cwd);
  if (!entry || activeCount(entry) === 0) return;
  computeNow(entry);
}

export function __resetPluginBubbleCacheForTest(): void {
  cache.clear();
  fetcher = defaultFetcher;
}
