import { useCallback, useEffect, useState } from "react";
import { webRequestContext } from "../../lib/webApi";
import { useAccountMeshClient } from "./accountMeshClient";
import { myMachinesView } from "./accountMachines";

export function useMeshConnections(enabled: boolean): number {
  const client = useAccountMeshClient();
  const generation = webRequestContext();
  const readSnapshot = useCallback(() => client.snapshot(), [client]);
  const subscribe = useCallback((onChange: () => void) => client.subscribe(onChange), [client]);
  const [summary, setSummary] = useState({ generation, count: 0 });
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    let running = false;
    let pending = false;
    const refresh = async () => {
      if (running) { pending = true; return; }
      running = true;
      do {
        pending = false;
        try {
          const state = await readSnapshot();
          const view = myMachinesView({ account: "signed_in", snapshot: state, now: Math.floor(Date.now() / 1000) });
          if (active) setSummary({ generation, count: view.kind === "account" ? view.machines.filter(peer => peer.connecting).length : 0 });
        } catch {
          if (active) setSummary({ generation, count: 0 });
        }
      } while (active && pending);
      running = false;
    };
    const unsubscribe = subscribe(() => { void refresh(); });
    void refresh();
    return () => { active = false; unsubscribe(); };
  }, [readSnapshot, subscribe, enabled, generation]);
  return enabled && summary.generation === generation ? summary.count : 0;
}
