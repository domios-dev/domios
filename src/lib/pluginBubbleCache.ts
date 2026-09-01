// Shared cache behind PluginBubbles (codeterm#336 part C): one entry per
// (pluginId, repository identity), computed only on a hidden->visible transition or an invalidation — no interval.
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
  /** Survives re-keying: the entry an alias collapses into replaces this. */
  holder: { entry: Entry };
}

interface Entry {
  pluginId: string;
  key: string | null;
  value: BubbleData | null | undefined;
  inFlight: boolean;
  pendingRefresh: boolean;
  subscribers: Map<string, Subscriber>;
}

const cache = new Map<string, Map<string | null, Entry>>();
/** Raw tab cwd → the daemon's canonical repository identity. */
const canonical = new Map<string, string>();

function keyFor(cwd: string | null): string | null {
  if (cwd === null) return null;
  return canonical.get(cwd) ?? cwd;
}

/** Collapse a cwd alias onto the identity the daemon resolved for it, moving
 * any entry already opened under the raw spelling. */
export function adoptRepoIdentity(cwd: string, identity: string): void {
  if (canonical.get(cwd) === identity) return;
  canonical.set(cwd, identity);
  if (cwd === identity) return;
  for (const byKey of cache.values()) rekeyEntry(byKey, cwd, identity);
}

function rekeyEntry(byKey: Map<string | null, Entry>, from: string, to: string): void {
  const source = byKey.get(from);
  if (!source) return;
  byKey.delete(from);
  const target = byKey.get(to);
  if (!target) {
    source.key = to;
    byKey.set(to, source);
    return;
  }
  const moved = [...source.subscribers.entries()];
  source.subscribers.clear();
  for (const [id, sub] of moved) {
    sub.holder.entry = target;
    target.subscribers.set(id, sub);
  }
  if (target.value === undefined) target.value = source.value;
  if (target.value !== undefined) for (const [, sub] of moved) sub.onChange(target.value ?? null);
}

function getOrCreateEntry(pluginId: string, key: string | null): Entry {
  let byKey = cache.get(pluginId);
  if (!byKey) {
    byKey = new Map();
    cache.set(pluginId, byKey);
  }
  let entry = byKey.get(key);
  if (!entry) {
    entry = { pluginId, key, value: undefined, inFlight: false, pendingRefresh: false, subscribers: new Map() };
    byKey.set(key, entry);
  }
  return entry;
}

function deleteEntryIfEmpty(entry: Entry): void {
  if (entry.subscribers.size > 0) return;
  const byKey = cache.get(entry.pluginId);
  if (byKey?.get(entry.key) === entry) byKey.delete(entry.key);
  if (byKey && byKey.size === 0) cache.delete(entry.pluginId);
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
  fetcher(entry.pluginId, entry.key, pickTabId(entry))
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
  const holder = { entry: getOrCreateEntry(pluginId, keyFor(cwd)) };
  const subscriberId = `${tabId}:${Math.random().toString(36).slice(2)}`;
  holder.entry.subscribers.set(subscriberId, { tabId, active: false, onChange, holder });

  if (holder.entry.value !== undefined) onChange(holder.entry.value);

  const setActive = (nextActive: boolean): void => {
    const sub = holder.entry.subscribers.get(subscriberId);
    if (!sub) return;
    const wasActive = sub.active;
    sub.active = nextActive;
    if (nextActive && !wasActive) computeNow(holder.entry);
  };
  setActive(active);

  return {
    setActive,
    unsubscribe: () => {
      holder.entry.subscribers.delete(subscriberId);
      deleteEntryIfEmpty(holder.entry);
    },
  };
}

/** Explicit invalidation, coalesced with any in-flight refresh; a no-op
 * unless a visible tab is watching this repo identity. */
export function invalidatePluginBubble(pluginId: string, cwd: string | null | undefined): void {
  if (cwd === undefined) return;
  const entry = cache.get(pluginId)?.get(keyFor(cwd));
  if (!entry || activeCount(entry) === 0) return;
  computeNow(entry);
}

/** The single action a repository-changed event takes: one computation per
 * plugin for that identity, however many tabs are watching it. */
export function invalidatePluginBubblesForRepo(identity: string): void {
  for (const byKey of cache.values()) {
    const entry = byKey.get(identity);
    if (entry && activeCount(entry) > 0) computeNow(entry);
  }
}

export function __resetPluginBubbleCacheForTest(): void {
  cache.clear();
  canonical.clear();
  fetcher = defaultFetcher;
}
