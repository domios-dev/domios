import { useEffect, useRef, useState } from "react";
import { publishNotice } from "../../lib/notices";
import { neighborClient, type NeighborClient } from "./neighborClient";
import { neighborName, type NeighborCommand, type NeighborList } from "./neighborModel";

interface LoadedNeighbors {
  identity: string | null;
  list: NeighborList | null;
  error: string | null;
}

export function useNeighbors(identity: string | null, client: NeighborClient = neighborClient) {
  const [loaded, setLoaded] = useState<LoadedNeighbors>({ identity, list: null, error: null });
  const [revision, setRevision] = useState(0);
  const [pending, setPending] = useState(false);
  const busy = useRef(false);
  const generation = useRef(0);
  const currentIdentity = useRef(identity);
  currentIdentity.current = identity;
  const seen = useRef(new Set<string>());

  useEffect(() => {
    seen.current = new Set();
    if (identity) {
      try {
        const stored: unknown = JSON.parse(localStorage.getItem(`domios.neighbors.seen:${identity}`) ?? "[]");
        if (Array.isArray(stored)) seen.current = new Set(stored.filter((id): id is string => typeof id === "string").slice(-200));
      } catch { seen.current = new Set(); }
    }
  }, [identity]);

  useEffect(() => {
    const epoch = ++generation.current;
    const controller = new AbortController();
    if (!identity) {
      setLoaded({ identity, list: null, error: null });
      return () => controller.abort();
    }
    let latest = 0;
    async function load() {
      const request = ++latest;
      try {
        const list = await client.list(controller.signal);
        if (controller.signal.aborted || generation.current !== epoch || request !== latest) return;
        setLoaded({ identity, list, error: null });
        for (const incoming of list.incoming) {
          if (seen.current.has(incoming.request_id)) continue;
          publishNotice({ kind: "toast", severity: "Info", id: `neighbor-request:${incoming.request_id}`,
            title: "New Neighbor request", detail: `${neighborName(incoming)} would like to connect. Open Neighbors in General to respond.` });
          seen.current.add(incoming.request_id);
        }
        seen.current = new Set([...seen.current].slice(-200));
        try { localStorage.setItem(`domios.neighbors.seen:${identity}`, JSON.stringify([...seen.current])); }
        catch { /* Notifications remain deduplicated for this session. */ }
      } catch (failure: unknown) {
        if (!controller.signal.aborted && generation.current === epoch && request === latest) {
          setLoaded(previous => ({ identity, list: previous.identity === identity ? previous.list : null,
            error: failure instanceof Error ? failure.message : "Neighbors unavailable" }));
        }
      }
    }
    void load();
    window.addEventListener("focus", load);
    return () => { controller.abort(); generation.current++; window.removeEventListener("focus", load); };
  }, [identity, client, revision]);

  async function apply(command: Exclude<NeighborCommand, { operation: "list" }>): Promise<boolean> {
    if (busy.current || !identity) return false;
    busy.current = true;
    setPending(true);
    try {
      await client.apply(command);
      if (currentIdentity.current !== identity) return false;
      setRevision(value => value + 1);
      return true;
    } catch (failure: unknown) {
      if (currentIdentity.current === identity) setLoaded(previous => ({ ...previous,
        error: failure instanceof Error ? failure.message : "Neighbors request failed" }));
      return false;
    } finally {
      busy.current = false;
      setPending(false);
    }
  }

  return { list: loaded.identity === identity ? loaded.list : null,
    error: loaded.identity === identity ? loaded.error : null, pending, apply,
    refresh: () => setRevision(value => value + 1) };
}
