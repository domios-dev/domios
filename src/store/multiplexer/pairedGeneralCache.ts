import { frontendCacheRepository, frontendCacheScope, type FrontendCacheRecord } from "../../lib/frontendCacheRecords";
import { tabSessionBound } from "../../lib/tabFailure";
import { isGeneralAgent } from "../../lib/roleKind";
import { parseCompositeTabId } from "../../lib/remoteTabTransport";
import type { Tab } from "../../lib/types";
import type { ObservedPeerTab } from "./pairedWorkspaceProjection";

export interface CachedPeerGeneral {
  publicKey: string;
  sessions: ObservedPeerTab[];
}

export const pairedGeneralCacheScope = (accountId: string) => frontendCacheScope(`codeterm:paired-general:${accountId}`);
const schemaVersion = 1;

export async function readPairedGeneralCache(scope: string): Promise<ReadonlyMap<string, CachedPeerGeneral>> {
  const snapshot = await frontendCacheRepository.read(scope).catch(() => null);
  const peers = new Map<string, CachedPeerGeneral>();
  if (snapshot?.manifest.metadata.schemaVersion !== schemaVersion) return peers;
  for (const record of snapshot.records) {
    const value = record.value as Partial<CachedPeerGeneral> | null;
    if (record.kind !== "peer" || typeof value?.publicKey !== "string" || !Array.isArray(value.sessions)) continue;
    const sessions = value.sessions.filter(session => {
      const identity = parseCompositeTabId(session?.viewerId);
      return session?.tab && identity?.peerId === record.id
        && identity.remoteTabId === session.tab.id && isGeneralAgent(session.tab);
    });
    peers.set(record.id, { publicKey: value.publicKey, sessions });
  }
  return peers;
}

function cachedSession({ viewerId, tab, status }: ObservedPeerTab): ObservedPeerTab {
  const {
    id, title, description, protected: protectedTab, tab_type, role, role_profile, role_label,
    agent_type, sort_order, frozen, slept_at_ms, boot_stage,
  } = tab;
  const row: Tab = {
    id, title, description, protected: protectedTab, tab_type, role, role_profile, role_label, agent_type,
    sort_order, frozen, slept_at_ms, boot_stage, session_bound: tabSessionBound(tab), cwd: null, pty_id: null,
  };
  return { viewerId, tab: row, status };
}

export async function writePairedGeneralCache(scope: string, peers: ReadonlyMap<string, CachedPeerGeneral>): Promise<void> {
  const records: FrontendCacheRecord[] = Array.from(peers, ([id, value]) => ({
    kind: "peer", id, value: { publicKey: value.publicKey, sessions: value.sessions.map(cachedSession) },
  }));
  await frontendCacheRepository.write(scope, records, { schemaVersion }).catch(() => {});
}
