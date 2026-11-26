import { useEffect, type RefObject } from "react";
import { invoke, listen } from "../../lib/bridge";
import { resumeRemoteOutputStreams } from "../../lib/outputStream";
import { isMeshDisabledError } from "../../lib/remoteChatState";
import { updateRemoteChatPeers } from "../../lib/remoteChatStatus";
import { resumeRemoteTabEvents } from "../../lib/remoteTabEvents";
import type { MeshPeersResponse } from "../../lib/types";
import type { AccountMeshSnapshot } from "../../components/MeshPanel/accountMeshClient";
import { rememberAccountMachineNames, rememberMachineIdentities } from "../../lib/useMeshMachineIdentities";

export interface PairedRoster {
  peers: MeshPeersResponse;
  revision: number;
}

function offlineMeshPeers(previous: MeshPeersResponse): MeshPeersResponse {
  return {
    ...previous,
    paired: previous.paired.map(peer => (peer.online ? { ...peer, online: false } : peer)),
    installations: previous.installations?.map(row => (row.online ? { ...row, online: false } : row)),
  };
}

export function useMeshPeerProjection(
  remoteProjectionActiveRef: RefObject<boolean>,
  hasPairedPeersRef: RefObject<boolean | null>,
  markRemoteChannelsReconnecting: () => void,
  requestRemoteRefresh: () => void,
  acceptPairedPeers: (roster: PairedRoster) => void,
  settleInitialRoster: () => void,
) {
  useEffect(() => {
    remoteProjectionActiveRef.current = true;
    let active = true;
    let peerReadGeneration = 0;
    let identityReadGeneration = 0;
    let lastPeers: MeshPeersResponse | null = null;
    let meshUnlisten: (() => void) | undefined;
    let identityUnlisten: (() => void) | undefined;
    const accountUnlisteners: Array<() => void> = [];
    let accountReadGeneration = 0;
    const refreshAccountNames = async () => {
      const generation = ++accountReadGeneration;
      try {
        const account = await invoke<AccountMeshSnapshot>("account_mesh_state");
        if (active && account && generation === accountReadGeneration) rememberAccountMachineNames(account);
      } catch {}
    };
    for (const event of ["account-changed", "account-mesh-changed", "mesh-state-changed"]) {
      void listen(event, refreshAccountNames).then(stop => {
        if (active) {
          accountUnlisteners.push(stop);
          if (event === "account-mesh-changed") void refreshAccountNames();
        } else stop();
      }).catch(() => {
        if (active && event === "account-mesh-changed") void refreshAccountNames();
      });
    }
    let muxUnlisten: (() => void) | undefined;
    const refreshPeers = async (identityOnly = false) => {
      const identityGeneration = ++identityReadGeneration;
      const generation = identityOnly ? peerReadGeneration : ++peerReadGeneration;
      if (!identityOnly) hasPairedPeersRef.current = null;
      let meshDisabled = false;
      try {
        const peers = await invoke<MeshPeersResponse>("mesh_list_peers");
        if (active && generation === peerReadGeneration && Array.isArray(peers?.paired)) {
          lastPeers = peers;
          if (identityGeneration === identityReadGeneration) rememberMachineIdentities(peers);
          if (!identityOnly) {
            hasPairedPeersRef.current = peers.paired.length > 0;
            updateRemoteChatPeers(peers);
            acceptPairedPeers({ peers, revision: generation });
            const onlinePeers = peers.paired.filter(peer => peer.online);
            if (onlinePeers.length > 0) {
              resumeRemoteOutputStreams();
              for (const peer of onlinePeers) resumeRemoteTabEvents(peer.instance_id);
            }
          }
        }
      } catch (error) {
        if (active && !identityOnly && generation === peerReadGeneration && isMeshDisabledError(error)) {
          meshDisabled = true;
          if (lastPeers) {
            const offline = offlineMeshPeers(lastPeers);
            lastPeers = offline;
            hasPairedPeersRef.current = offline.paired.length > 0;
            updateRemoteChatPeers(offline);
            acceptPairedPeers({ peers: offline, revision: generation });
          }
        }
      }
      if (!identityOnly && active && generation === peerReadGeneration) {
        settleInitialRoster();
        if (!meshDisabled) void requestRemoteRefresh();
      }
    };
    const onMeshStateChanged = () => {
      markRemoteChannelsReconnecting();
      void refreshPeers();
    };
    void listen("mesh-state-changed", onMeshStateChanged).then((fn) => {
      if (active) {
        meshUnlisten = fn;
        void refreshPeers();
      } else fn();
    }).catch(() => { if (active) void refreshPeers(); });
    void listen("mesh-identity-changed", () => { void refreshPeers(true); }).then((fn) => {
      if (active) identityUnlisten = fn;
      else fn();
    });
    void listen("remote-workers-changed", () => {
      void requestRemoteRefresh();
    }).then((fn) => {
      if (active) muxUnlisten = fn;
      else fn();
    });
    return () => {
      active = false;
      remoteProjectionActiveRef.current = false;
      meshUnlisten?.();
      identityUnlisten?.();
      muxUnlisten?.();
      accountUnlisteners.forEach(stop => stop());
    };
  }, [remoteProjectionActiveRef, hasPairedPeersRef, markRemoteChannelsReconnecting, requestRemoteRefresh, acceptPairedPeers, settleInitialRoster]);
}
