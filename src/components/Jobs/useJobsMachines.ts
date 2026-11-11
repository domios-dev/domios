import { useEffect, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import { DEV_BUILD } from "../../lib/devBuild";
import type { MeshPeersResponse } from "../../lib/types";
import { readWhenMuxCapacityAvailable } from "../../lib/streamMuxRequestModel";

export function useJobsMachines(enabled = true) {
  const [state, setState] = useState<{ peers: MeshPeersResponse["paired"]; failed: boolean }>({ peers: [], failed: false });
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    let sequence = 0;
    let off: (() => void) | undefined;
    let controller: AbortController | undefined;
    const refresh = async () => {
      const read = ++sequence;
      controller?.abort();
      const current = new AbortController();
      controller = current;
      try {
        const result = await readWhenMuxCapacityAvailable(() => invoke<MeshPeersResponse>("mesh_list_peers"), current.signal);
        if (!Array.isArray(result.paired)) throw new Error("Peer list unavailable");
        if (active && read === sequence) setState({ peers: result.paired, failed: false });
      } catch (error) {
        if (!active || read !== sequence) return;
        if (DEV_BUILD) console.debug("Background paired machine refresh failed", error);
        setState((current) => ({ ...current, failed: true }));
      }
    };
    void refresh();
    void listen("mesh-state-changed", refresh).then((stop) => { if (active) off = stop; else stop(); }, () => undefined);
    return () => { active = false; controller?.abort(); off?.(); };
  }, [enabled]);
  return state;
}
