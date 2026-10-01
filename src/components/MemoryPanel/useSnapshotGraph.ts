import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import type { ApiConfig } from "../../lib/types";
import {
  SnapshotUnavailableError,
  fetchSnapshot,
  fetchSnapshotDelta,
} from "./memoryApi";
import { clearCachedSnapshot, loadCachedSnapshot, saveCachedSnapshot } from "./snapshotCache";
import {
  applyDelta,
  indexSnapshot,
  type NodeIndex,
} from "./snapshotLogic";
import { isFullSnapshot, isUnchanged, type GraphSnapshot } from "./snapshotTypes";

const ACTIVITY_DEBOUNCE_MS = 800;

export type SnapshotStatus = "loading" | "ready" | "unavailable";

export interface UseSnapshotGraphResult {
  index: NodeIndex | null;
  status: SnapshotStatus;
  stats: GraphSnapshot["stats"] | null;
  error: string | null;
  // Bumps on every full snapshot, so lazy per-node caches know to drop.
  epoch: number;
  refresh: () => Promise<void>;
}

export function useSnapshotGraph(api: ApiConfig): UseSnapshotGraphResult {
  const [index, setIndex] = useState<NodeIndex | null>(null);
  const [status, setStatus] = useState<SnapshotStatus>("loading");
  const [stats, setStats] = useState<GraphSnapshot["stats"] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [epoch, setEpoch] = useState(0);
  const indexRef = useRef<NodeIndex | null>(null);
  indexRef.current = index;
  const apiRef = useRef(api);
  apiRef.current = api;

  // Hard refresh: drop the IndexedDB cache and pull a fresh full snapshot.
  const refresh = useCallback(async () => {
    await clearCachedSnapshot();
    try {
      const snap = await fetchSnapshot(apiRef.current);
      setIndex(indexSnapshot(snap));
      setStats(snap.stats);
      setEpoch((e) => e + 1);
      setStatus("ready");
      setError(null);
      void saveCachedSnapshot(snap);
    } catch (e) {
      if (e instanceof SnapshotUnavailableError) setStatus("unavailable");
      else setError(String(e));
    }
  }, []);

  useEffect(() => {
    let active = true;
    const ctrl = new AbortController();

    const ingestFull = (snap: GraphSnapshot) => {
      if (!active) return;
      setIndex(indexSnapshot(snap));
      setStats(snap.stats);
      setEpoch((e) => e + 1);
      setStatus("ready");
      setError(null);
      void saveCachedSnapshot(snap);
    };

    const boot = async () => {
      const cached = await loadCachedSnapshot();
      if (!active) return;
      // The cache paints instantly; deltas carry only moved positions, so a full fetch follows.
      if (cached) {
        setIndex(indexSnapshot(cached));
        setStats(cached.stats);
        setStatus("ready");
      }
      try {
        const snap = await fetchSnapshot(apiRef.current, ctrl.signal);
        ingestFull(snap);
      } catch (e) {
        if ((e as Error)?.name === "AbortError") return;
        if (e instanceof SnapshotUnavailableError) {
          if (active) setStatus("unavailable");
          return;
        }
        if (active && !cached) {
          setStatus("loading");
          setError(String(e));
        }
      }
    };
    void boot();

    return () => {
      active = false;
      ctrl.abort();
    };
  }, [api.host, api.port, api.token]);

  // mem-activity → debounced delta fetch (same pattern as legacy useGraphData).
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    let debounce: ReturnType<typeof setTimeout> | null = null;

    const refreshDelta = async () => {
      const cur = indexRef.current;
      if (!cur) return;
      const ctrl = new AbortController();
      try {
        const delta = await fetchSnapshotDelta(apiRef.current, cur.layoutVersion, cur.seq, ctrl.signal);
        if (!active || isUnchanged(delta)) return;
        if (isFullSnapshot(delta)) {
          setIndex(indexSnapshot(delta));
          setStats(delta.stats);
          setEpoch((e) => e + 1);
          void saveCachedSnapshot(delta);
          return;
        }
        setIndex(applyDelta(cur, delta));
      } catch {
        /* transient — next event retries */
      }
    };

    const debounced = () => {
      if (debounce != null) clearTimeout(debounce);
      debounce = setTimeout(() => void refreshDelta(), ACTIVITY_DEBOUNCE_MS);
    };

    listen<{ op: string }>("mem-activity", (ev) => {
      if (!active) return;
      const op = ev.payload?.op;
      if (op === "write" || op === "update" || op === "delete") debounced();
    })
      .then((fn) => {
        if (!active) { fn(); return; }
        unlisten = fn;
      })
      .catch(() => {});

    return () => {
      active = false;
      if (unlisten) unlisten();
      if (debounce != null) clearTimeout(debounce);
    };
  }, [api.host, api.port, api.token]);

  return { index, status, stats, error, epoch, refresh };
}
