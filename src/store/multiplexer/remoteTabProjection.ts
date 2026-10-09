import type { Tab } from "../../lib/types";

export function shouldRefreshRemoteTabs(
  hasPairedPeers: boolean | null,
  ...tabs: Readonly<Record<string, Tab>>[]
): boolean {
  return hasPairedPeers !== false
    || tabs.some((record) => Object.values(record).some((tab) => Boolean(tab.remote_peer_id)));
}

export function remoteTabsFromOrdinaryProjection(tabs: Tab[]): Record<string, Tab> {
  return Object.fromEntries(
    tabs.filter((tab) => Boolean(tab.remote_peer_id)).map((tab) => [tab.id, tab]),
  );
}
