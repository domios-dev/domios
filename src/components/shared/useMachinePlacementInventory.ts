import { useEffect, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import type { MeshPeersResponse } from "../../lib/types";
import type { MeshIdentityProjection } from "../../lib/machineIdentity";
import { machinePlacementChoices, type MachinePlacementChoice } from "../../lib/machinePlacement";
import { rememberMachineIdentities } from "../../lib/useMeshMachineIdentities";

export function useMachinePlacementInventory(enabled = true) {
  const [choices, setChoices] = useState<MachinePlacementChoice[] | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!enabled) return;
    let active = true;
    let generation = 0;
    let stop: (() => void) | undefined;
    const refresh = async () => {
      const current = ++generation;
      try {
        const response = await invoke<MeshPeersResponse & MeshIdentityProjection>("mesh_list_peers");
        if (!active || current !== generation || !Array.isArray(response?.paired)) return;
        rememberMachineIdentities(response);
        setChoices(machinePlacementChoices(response));
        setFailed(false);
      } catch {
        if (active && current === generation) setFailed(true);
      }
    };
    void refresh();
    void listen("mesh-state-changed", refresh).then((unlisten) => {
      if (active) stop = unlisten;
      else unlisten();
    }, () => undefined);
    return () => {
      active = false;
      generation += 1;
      stop?.();
    };
  }, [enabled]);

  return { choices, failed };
}
