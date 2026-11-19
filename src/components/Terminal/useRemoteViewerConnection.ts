import { parseCompositeTabId } from "../../lib/remoteTabTransport";
import { remoteMachineName } from "../../lib/remoteChatState";
import type { Tab } from "../../lib/types";
import { usePairedWorkspaceInventory } from "../../store/multiplexer/context";
import { useRemoteChatStatus } from "../../lib/remoteChatStatus";
import { remoteViewerConnectionState, remoteViewerObservation, remoteViewerSendUnavailableReason } from "./remoteViewerStatus";

export function useRemoteViewerConnection(tabId: string, tab: Tab | null | undefined) {
  const peerId = parseCompositeTabId(tabId)?.peerId ?? tab?.remote_peer_id ?? null;
  const inventory = usePairedWorkspaceInventory();
  const observation = remoteViewerObservation(inventory.generals, inventory.browse, tabId, peerId);
  const remote = useRemoteChatStatus(tabId, peerId);
  const connectionState = peerId
    ? remoteViewerConnectionState(observation?.availability, tab?.remote_channels?.chat, remote.state)
    : null;
  const state = connectionState === "reconnecting" ? null : connectionState;
  const machine = remoteMachineName(peerId ?? "", observation?.name, remote.peer?.name, tab?.remote_peer_name);

  return {
    peerId,
    peer: remote.peer,
    failure: remote.failure,
    state,
    connectionState,
    observation,
    machine,
    sendUnavailableReason: remoteViewerSendUnavailableReason(connectionState, machine),
  };
}

export type RemoteViewerConnection = ReturnType<typeof useRemoteViewerConnection>;
