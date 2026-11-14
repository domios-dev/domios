import type { Tab } from "../../lib/types";
import type { RemotePeerTabs } from "../../lib/remoteTabPortal";

export function shouldRefreshRemoteTabs(
  hasPairedPeers: boolean | null,
  ...tabs: Readonly<Record<string, Tab>>[]
): boolean {
  return hasPairedPeers !== false
    || tabs.some((record) => Object.values(record).some((tab) => Boolean(tab.remote_peer_id)));
}

/** The local fields the ordinary projection copies into remote viewers: each controller's title, workspace and team. */
export function remoteControllerSignature(tabs: Readonly<Record<string, Tab>>): string {
  const controllers = new Set<string>();
  for (const tab of Object.values(tabs)) {
    if (tab.remote_peer_id && tab.parent_tab_id) controllers.add(tab.parent_tab_id);
  }
  return JSON.stringify([...controllers].sort().map((id) => {
    const controller = tabs[id];
    return [id, controller?.title ?? null, controller?.workspace_id ?? null, controller?.team_id ?? null];
  }));
}

export function remoteTabsFromOrdinaryProjection(tabs: Tab[]): Record<string, Tab> {
  return Object.fromEntries(
    tabs.filter((tab) => Boolean(tab.remote_peer_id)).map((tab) => [tab.id, tab]),
  );
}

/** The peer's own record decides each attachment's agent state and whether its PTY exists yet. */
export function applyRemotePeerAgentStates(
  projected: Record<string, Tab>,
  peer: RemotePeerTabs,
): Record<string, Tab> {
  const tabs = { ...projected };
  for (const source of peer.tabs) {
    const id = `mesh:${peer.peer_id}/${source.id}`;
    const tab = tabs[id];
    if (tab?.remote_peer_id !== peer.peer_id || tab.remote_tab_id !== source.id) continue;
    tabs[id] = { ...tab, agent_state: source.agent_state, pty_id: source.pty_id ? id : null };
  }
  return tabs;
}

/** An attachment whose tab the answering peer no longer lists is gone there, so it leaves here too. */
export function withoutVanishedAttachments(
  projected: Record<string, Tab>,
  attachments: readonly Tab[],
  peer: RemotePeerTabs,
): Record<string, Tab> {
  const listed = new Set(peer.tabs.map((source) => source.id));
  const tabs = { ...projected };
  for (const tab of attachments) {
    if (tab.remote_peer_id === peer.peer_id && !listed.has(tab.remote_tab_id ?? "")) delete tabs[tab.id];
  }
  return tabs;
}

export async function refreshAttachedRemoteAgentStates(
  projected: Record<string, Tab>,
  ordinary: Record<string, Tab>,
  readPeer: (peerId: string) => Promise<RemotePeerTabs>,
): Promise<Record<string, Tab>> {
  const attachments = Object.values(projected).filter((tab) => !ordinary[tab.id] && tab.remote_peer_id);
  const peerIds = [...new Set(attachments.map((tab) => tab.remote_peer_id!))];
  const peers = await Promise.allSettled(peerIds.map(readPeer));
  let tabs = projected;
  for (const peer of peers) {
    if (peer.status !== "fulfilled") continue;
    tabs = withoutVanishedAttachments(applyRemotePeerAgentStates(tabs, peer.value), attachments, peer.value);
  }
  return tabs;
}
