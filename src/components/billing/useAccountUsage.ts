import { useContext, useEffect, useState, useSyncExternalStore } from "react";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import { localFetch } from "../../lib/webApi";
import { listen } from "../../lib/bridge";
import { useAccountMeshClient } from "../MeshPanel/accountMeshClient";
import { limitCounts } from "./freeLimitModel";

export interface AccountUsage {
  workspaces?: number;
  teams?: number;
  "agent.tabs"?: number;
  "mesh.machines"?: number;
  "mem.items"?: number;
}

const subscribeEmpty = () => () => {};
const emptySnapshot = () => null;

export function useAccountUsage(identity: string | null): AccountUsage {
  const store = useContext(MuxStateStoreContext);
  const state = useSyncExternalStore(store?.subscribe ?? subscribeEmpty, store?.getState ?? emptySnapshot);
  const client = useAccountMeshClient();
  const [loaded, setLoaded] = useState<{ identity: string | null; usage: AccountUsage } | null>(null);
  useEffect(() => {
    let active = true;
    let revision = 0;
    const load = async () => {
      const request = ++revision;
      const [memory, mesh] = await Promise.allSettled([
        localFetch("/api/mem/stats").then(async response => {
          if (!response.ok) throw new Error("Memory usage unavailable");
          return await response.json() as { total_items?: unknown };
        }),
        identity ? client.snapshot() : Promise.resolve(null),
      ]);
      if (!active || request !== revision) return;
      const items = memory.status === "fulfilled" ? memory.value.total_items : undefined;
      const snapshot = mesh.status === "fulfilled" ? mesh.value : null;
      const registry = snapshot?.binding === "current" && !snapshot.registry_failure ? snapshot.registry : null;
      setLoaded({ identity, usage: {
        ...(typeof items === "number" && Number.isSafeInteger(items) && items >= 0 ? { "mem.items": items } : {}),
        ...(registry && !registry.cursor ? { "mesh.machines": registry.rows.filter(row => row.active).length } : {}),
      } });
    };
    void load();
    const unsubscribe = client.subscribe(() => { void load(); });
    window.addEventListener("focus", load);
    let unlisten: (() => void) | undefined;
    void listen("mem-activity", () => { void load(); }).then(fn => {
      if (active) unlisten = fn;
      else fn();
    }).catch(() => undefined);
    return () => { active = false; unsubscribe(); unlisten?.(); window.removeEventListener("focus", load); };
  }, [client, identity]);
  return { ...(state && state.generation > 0 ? limitCounts(state) : {}), ...(loaded?.identity === identity ? loaded.usage : {}) };
}
