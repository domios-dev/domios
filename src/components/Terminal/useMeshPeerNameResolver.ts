import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import type { MeshPeersResponse } from "../../lib/types";

const EMPTY_NAMES = new Map<string, string>();
let cachedNames: Map<string, string> | null = null;
let pendingNames: Promise<Map<string, string>> | null = null;

function peerNames(response: MeshPeersResponse): Map<string, string> {
  const names = new Map<string, string>();
  for (const peer of [...response.paired, ...response.discovered]) {
    names.set(peer.instance_id, peer.instance_name);
    names.set(peer.instance_name.toLowerCase(), peer.instance_name);
  }
  return names;
}

function loadPeerNames(): Promise<Map<string, string>> {
  if (cachedNames) return Promise.resolve(cachedNames);
  pendingNames ??= invoke<MeshPeersResponse>("mesh_list_peers")
    .then((response) => {
      cachedNames = peerNames(response);
      return cachedNames;
    })
    .finally(() => {
      pendingNames = null;
    });
  return pendingNames;
}

export function useMeshPeerNameResolver(active: boolean): (peerRef: string) => string | undefined {
  const [names, setNames] = useState(cachedNames ?? EMPTY_NAMES);

  useEffect(() => {
    if (!active) return;
    let mounted = true;
    void loadPeerNames().then((loaded) => {
      if (mounted) setNames(loaded);
    }).catch(() => undefined);
    return () => {
      mounted = false;
    };
  }, [active]);

  return useCallback(
    (peerRef: string) => names.get(peerRef) ?? names.get(peerRef.toLowerCase()),
    [names],
  );
}
