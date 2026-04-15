import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ApiConfig } from "../../lib/types";
import {
  fetchGraphFull,
  fetchPinnedPositions,
  isTauriEnv,
  pinPinnedPosition,
  unpinPinnedPosition,
} from "./memoryApi";
import type { MemEdge, MemNode } from "./types";

interface State {
  nodes: Map<string, MemNode>;
  edges: MemEdge[];
  loading: boolean;
  error: string | null;
  stats: { node_count: number; edge_count: number };
  pinnedPositions: Map<string, { x: number; y: number }>;
}

export function useGraphData(api: ApiConfig) {
  const [state, setState] = useState<State>({
    nodes: new Map(),
    edges: [],
    loading: true,
    error: null,
    stats: { node_count: 0, edge_count: 0 },
    pinnedPositions: new Map(),
  });

  // Initial fetch: graph + pinned positions in parallel. Positions are now
  // owned by the layout worker; this hook only seeds user-anchored ones.
  useEffect(() => {
    const ctrl = new AbortController();

    Promise.all([
      fetchGraphFull(api, ctrl.signal),
      fetchPinnedPositions(api, ctrl.signal).catch(() => []),
    ])
      .then(([graph, pinned]) => {
        if (ctrl.signal.aborted) return;
        const nodes = new Map<string, MemNode>();
        graph.nodes.forEach((n) => nodes.set(n.id, n));
        const pinnedPositions = new Map<string, { x: number; y: number }>();
        for (const p of pinned) pinnedPositions.set(p.mem_id, { x: p.x, y: p.y });
        setState({
          nodes,
          edges: graph.edges,
          loading: false,
          error: null,
          stats: graph.stats,
          pinnedPositions,
        });
      })
      .catch((e) => {
        if (e?.name === "AbortError") return;
        setState((s) => ({ ...s, loading: false, error: String(e) }));
      });

    return () => ctrl.abort();
  }, [api]);

  const updateNode = useCallback(
    (id: string, patch: Partial<MemNode>) => {
      setState((prev) => {
        const cur = prev.nodes.get(id);
        if (!cur) return prev;
        const next = new Map(prev.nodes);
        next.set(id, { ...cur, ...patch });
        return { ...prev, nodes: next };
      });
    },
    [],
  );

  const removeNode = useCallback((id: string) => {
    setState((prev) => {
      if (!prev.nodes.has(id)) return prev;
      const nodes = new Map(prev.nodes);
      nodes.delete(id);
      const edges = prev.edges.filter((e) => e.from !== id && e.to !== id);
      const pinnedPositions = new Map(prev.pinnedPositions);
      pinnedPositions.delete(id);
      return {
        ...prev,
        nodes,
        edges,
        pinnedPositions,
        stats: {
          node_count: Math.max(0, prev.stats.node_count - 1),
          edge_count: edges.length,
        },
      };
    });
  }, []);

  const pinPosition = useCallback(
    (id: string, x: number, y: number) => {
      setState((prev) => {
        if (!prev.nodes.has(id)) return prev;
        const pinnedPositions = new Map(prev.pinnedPositions);
        pinnedPositions.set(id, { x, y });
        return { ...prev, pinnedPositions };
      });
      pinPinnedPosition(api, id, x, y).catch(() => {});
    },
    [api],
  );

  const unpinPosition = useCallback(
    (id: string) => {
      setState((prev) => {
        if (!prev.pinnedPositions.has(id)) return prev;
        const pinnedPositions = new Map(prev.pinnedPositions);
        pinnedPositions.delete(id);
        return { ...prev, pinnedPositions };
      });
      unpinPinnedPosition(api, id).catch(() => {});
    },
    [api],
  );

  // Event-driven refresh: any write to mem → refetch graph.
  // Positions stay: the worker re-seeds only truly-new nodes.
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    let debounceTimer: ReturnType<typeof setTimeout> | null = null;

    const refresh = () => {
      const ctrl = new AbortController();
      fetchGraphFull(api, ctrl.signal)
        .then((resp) => {
          if (!active || ctrl.signal.aborted) return;
          setState((prev) => {
            const next = new Map<string, MemNode>();
            for (const n of resp.nodes) {
              // Preserve any local updates that haven't been persisted yet
              const prior = prev.nodes.get(n.id);
              next.set(n.id, prior ?? n);
            }
            return { ...prev, nodes: next, edges: resp.edges, stats: resp.stats };
          });
        })
        .catch(() => {});
    };

    const debouncedRefresh = () => {
      if (debounceTimer != null) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(refresh, 800);
    };

    if (isTauriEnv()) {
      listen<{ op: string }>("mem-activity", (ev) => {
        if (!active) return;
        const op = ev.payload?.op;
        if (op === "write" || op === "update" || op === "delete") {
          debouncedRefresh();
        }
      })
        .then((fn) => {
          if (!active) { fn(); return; }
          unlisten = fn;
        })
        .catch(() => {});
    }

    return () => {
      active = false;
      if (unlisten) unlisten();
      if (debounceTimer != null) clearTimeout(debounceTimer);
    };
  }, [api]);

  return {
    ...state,
    updateNode,
    removeNode,
    pinPosition,
    unpinPosition,
  };
}
