import type { Tab } from "./types";
import { frontendCacheRepository, frontendCacheScope, type FrontendCacheRecord } from "./frontendCacheRecords";

export interface RemotePeerTabs {
  peer_id: string;
  peer_name: string;
  tabs: Tab[];
}

const CACHE_SCOPE = "codeterm:remote-tab-attachments:v1";
const CACHE_SCHEMA = 1;

interface AttachmentCache {
  tabs: Record<string, Tab>;
  hydrated: boolean;
  hydration?: Promise<Record<string, Tab>>;
  pendingUpserts: Record<string, Tab>;
  pendingDeletes: Set<string>;
  dismissed: Set<string>;
  pendingVisibility: Map<string, boolean>;
}

const attachmentCaches = new Map<string, AttachmentCache>();

function cacheScope(): string | null {
  return frontendCacheScope(CACHE_SCOPE);
}

function cacheFor(scope: string): AttachmentCache {
  let cache = attachmentCaches.get(scope);
  if (!cache) {
    cache = { tabs: {}, hydrated: false, pendingUpserts: {}, pendingDeletes: new Set(), dismissed: new Set(), pendingVisibility: new Map() };
    attachmentCaches.set(scope, cache);
  }
  return cache;
}

function validAttachments(records: FrontendCacheRecord[], schemaVersion: unknown): Record<string, Tab> | null {
  if (schemaVersion !== CACHE_SCHEMA || records.some((record) => record.kind !== "attachment" && record.kind !== "dismissed")) return null;
  const tabs: Record<string, Tab> = {};
  for (const record of records) {
    if (record.kind !== "attachment") continue;
    const tab = record.value as Tab | null;
    if (!tab || typeof tab !== "object" || record.id !== tab.id || !record.id.startsWith("mesh:")) return null;
    tabs[record.id] = tab;
  }
  return tabs;
}

function attachmentRecords(tabs: Record<string, Tab>): FrontendCacheRecord[] {
  return Object.entries(tabs).map(([id, tab]) => ({ kind: "attachment", id, value: tab }));
}

async function persistAttachments(scope: string, cache: AttachmentCache): Promise<void> {
  const records = [...attachmentRecords(cache.tabs),
    ...[...cache.dismissed].map(id => ({ kind: "dismissed", id, value: null }))];
  await frontendCacheRepository.write(scope, records, { schemaVersion: CACHE_SCHEMA });
}

async function hydrateScope(scope: string): Promise<Record<string, Tab>> {
  const cache = cacheFor(scope);
  if (cache.hydrated) return cache.tabs;
  if (cache.hydration) return cache.hydration;
  const hydration = frontendCacheRepository.read(scope).then(async (snapshot) => {
    const stored = snapshot
      ? validAttachments(snapshot.records, snapshot.manifest.metadata.schemaVersion)
      : {};
    if (stored === null) {
      if (snapshot) await frontendCacheRepository.clear(scope, snapshot.manifest.revision);
      cache.tabs = {};
    } else {
      cache.tabs = { ...stored, ...cache.tabs };
      for (const record of snapshot?.records ?? []) {
        if (record.kind === "dismissed" && record.id.startsWith("mesh:")) cache.dismissed.add(record.id);
      }
    }
    for (const id of cache.pendingDeletes) delete cache.tabs[id];
    Object.assign(cache.tabs, cache.pendingUpserts);
    for (const [id, dismissed] of cache.pendingVisibility) {
      if (dismissed) cache.dismissed.add(id);
      else cache.dismissed.delete(id);
    }
    const changedBeforeHydration = cache.pendingDeletes.size > 0 || Object.keys(cache.pendingUpserts).length > 0 || cache.pendingVisibility.size > 0;
    cache.pendingVisibility.clear();
    cache.pendingDeletes.clear();
    cache.pendingUpserts = {};
    cache.hydrated = true;
    if (changedBeforeHydration) await persistAttachments(scope, cache);
    return cache.tabs;
  }).catch(() => cache.tabs).finally(() => {
    cache.hydration = undefined;
  });
  cache.hydration = hydration;
  return hydration;
}

export function reconcileAttachedRemoteTabs(
  live: Record<string, Tab>,
  attached: Record<string, Tab>,
): Record<string, Tab> {
  const tabs = { ...attached, ...live };
  for (const [id, attachment] of Object.entries(attached)) {
    if (live[id]) tabs[id] = { ...live[id], workspace_id: attachment.workspace_id };
  }
  return tabs;
}

export function loadAttachedRemoteTabs(): Record<string, Tab> {
  const scope = cacheScope();
  return scope ? cacheFor(scope).tabs : {};
}

export function hydrateAttachedRemoteTabs(): Promise<Record<string, Tab>> {
  const scope = cacheScope();
  return scope ? hydrateScope(scope) : Promise.resolve({});
}

export function saveAttachedRemoteTabs(tabs: Record<string, Tab>): void {
  const scope = cacheScope();
  if (!scope) return;
  const cache = cacheFor(scope);
  const valid = Object.fromEntries(Object.entries(tabs).filter(([id, tab]) => id === tab.id && id.startsWith("mesh:")));
  const previousIds = Object.keys(cache.tabs);
  cache.tabs = valid;
  if (!cache.hydrated) {
    for (const [id, tab] of Object.entries(valid)) {
      cache.pendingUpserts[id] = tab;
      cache.pendingDeletes.delete(id);
    }
    for (const id of previousIds) {
      if (!Object.prototype.hasOwnProperty.call(valid, id)) {
        delete cache.pendingUpserts[id];
        cache.pendingDeletes.add(id);
      }
    }
    void hydrateScope(scope);
    return;
  }
  void persistAttachments(scope, cache).catch(() => undefined);
}

export function isRemoteTabDismissed(tabId: string): boolean {
  if (!tabId.startsWith("mesh:")) return false;
  const scope = cacheScope();
  return scope ? cacheFor(scope).dismissed.has(tabId) : false;
}

export function visibleRemoteTabs(tabs: Record<string, Tab>): Record<string, Tab> {
  const scope = cacheScope();
  const dismissed = scope && cacheFor(scope).dismissed;
  if (!dismissed || dismissed.size === 0) return tabs;
  return Object.fromEntries(Object.entries(tabs).filter(([id]) => !dismissed.has(id)));
}

export function setRemoteTabsDismissed(ids: readonly string[], dismissed: boolean): void {
  const scope = cacheScope();
  if (!scope) return;
  const cache = cacheFor(scope);
  let changed = false;
  for (const id of ids) {
    if (cache.hydrated && cache.dismissed.has(id) === dismissed) continue;
    changed = true;
    if (dismissed) cache.dismissed.add(id);
    else cache.dismissed.delete(id);
    if (!cache.hydrated) cache.pendingVisibility.set(id, dismissed);
  }
  if (!changed) return;
  if (cache.hydrated) void persistAttachments(scope, cache).catch(() => {});
  else void hydrateScope(scope);
}

export function projectAttachedRemoteTab(
  source: Tab,
  peer: Pick<RemotePeerTabs, "peer_id" | "peer_name">,
  workspaceId: string,
): Tab {
  const ordinaryId = `mesh:${peer.peer_id}/${source.id}`;
  return {
    ...source,
    id: ordinaryId,
    workspace_id: workspaceId,
    pty_id: source.pty_id ? ordinaryId : null,
    remote_pty_id: source.pty_id ?? null,
    remote_peer_id: peer.peer_id,
    remote_peer_name: peer.peer_name,
    remote_tab_id: source.id,
    remote_channels: { control: "available", output: "available", chat: "available" },
    ownership: undefined,
    parent_tab_id: null,
    spawned_by_tab_id: null,
    team_id: null,
  };
}
