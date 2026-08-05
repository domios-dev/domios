import type { Tab } from "../../lib/types";

export function remoteTabsFromOrdinaryProjection(tabs: Tab[]): Record<string, Tab> {
  return Object.fromEntries(
    tabs.filter((tab) => Boolean(tab.remote_peer_id)).map((tab) => [tab.id, tab]),
  );
}
