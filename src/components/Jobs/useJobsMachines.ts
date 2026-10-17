import { useEffect, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import type { MeshPeersResponse } from "../../lib/types";

export function useJobsMachines(enabled = true) {
  const [state, setState] = useState<{ peers: MeshPeersResponse["paired"]; failed: boolean }>({ peers: [], failed: false });
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    let sequence = 0;
    let off: (() => void) | undefined;
    const refresh = async () => {
      const read = ++sequence;
      try {
        const result = await invoke<MeshPeersResponse>("mesh_list_peers");
        if (!Array.isArray(result.paired)) throw new Error("Peer list unavailable");
        if (active && read === sequence) setState({ peers: result.paired, failed: false });
      } catch {
        if (active && read === sequence) setState((current) => ({ ...current, failed: true }));
      }
    };
    void refresh();
    void listen("mesh-state-changed", refresh).then((stop) => { if (active) off = stop; else stop(); }, () => undefined);
    return () => { active = false; off?.(); };
  }, [enabled]);
  return state;
}
