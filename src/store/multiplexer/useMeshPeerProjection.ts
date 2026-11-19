import { useEffect, type RefObject } from "react";
import { invoke, listen } from "../../lib/bridge";
import { updateRemoteChatPeers } from "../../lib/remoteChatStatus";
import type { MeshPeersResponse } from "../../lib/types";
import { rememberMachineIdentities } from "../../lib/useMeshMachineIdentities";

export function useMeshPeerProjection(
  remoteProjectionActiveRef: RefObject<boolean>,
  hasPairedPeersRef: RefObject<boolean | null>,
  markRemoteChannelsReconnecting: () => void,
  requestRemoteRefresh: () => void,
) {
  useEffect(() => {
    remoteProjectionActiveRef.current = true;
    let active = true;
    let peerReadGeneration = 0;
    let identityReadGeneration = 0;
    let meshUnlisten: (() => void) | undefined;
    let identityUnlisten: (() => void) | undefined;
    let muxUnlisten: (() => void) | undefined;
    const refreshPeers = async (identityOnly = false) => {
      const identityGeneration = ++identityReadGeneration;
      const generation = identityOnly ? peerReadGeneration : ++peerReadGeneration;
      if (!identityOnly) hasPairedPeersRef.current = null;
      try {
        const peers = await invoke<MeshPeersResponse>("mesh_list_peers");
        if (active && generation === peerReadGeneration && Array.isArray(peers?.paired)) {
          if (identityGeneration === identityReadGeneration) rememberMachineIdentities(peers);
          if (!identityOnly) {
            hasPairedPeersRef.current = peers.paired.length > 0;
            updateRemoteChatPeers(peers);
          }
        }
      } catch {}
      if (!identityOnly && active && generation === peerReadGeneration) void requestRemoteRefresh();
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
    });
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
    };
  }, [remoteProjectionActiveRef, hasPairedPeersRef, markRemoteChannelsReconnecting, requestRemoteRefresh]);
}
