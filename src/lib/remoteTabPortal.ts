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
}

const attachmentCaches = new Map<string, AttachmentCache>();

function cacheScope(): string | null {
  return frontendCacheScope(CACHE_SCOPE);
}

function cacheFor(scope: string): AttachmentCache {
  let cache = attachmentCaches.get(scope);
  if (!cache) {
    cache = { tabs: {}, hydrated: false, pendingUpserts: {}, pendingDeletes: new Set() };
    attachmentCaches.set(scope, cache);
  }
  return cache;
}

function validAttachments(records: FrontendCacheRecord[], schemaVersion: unknown): Record<string, Tab> | null {
  if (schemaVersion !== CACHE_SCHEMA || records.some((record) => record.kind !== "attachment")) return null;
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
  await frontendCacheRepository.write(scope, attachmentRecords(cache.tabs), { schemaVersion: CACHE_SCHEMA });
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
    }
    for (const id of cache.pendingDeletes) delete cache.tabs[id];
    Object.assign(cache.tabs, cache.pendingUpserts);
    const changedBeforeHydration = cache.pendingDeletes.size > 0 || Object.keys(cache.pendingUpserts).length > 0;
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
