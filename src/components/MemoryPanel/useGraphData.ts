import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ApiConfig } from "../../lib/types";
import { fetchGraphFull, patchPositions, isTauriEnv } from "./memoryApi";
import { forceLayout } from "./forceLayout";
import type { MemEdge, MemNode } from "./types";

interface State {
  nodes: Map<string, MemNode>;
  edges: MemEdge[];
  loading: boolean;
  error: string | null;
  stats: { node_count: number; edge_count: number };
}

export function useGraphData(api: ApiConfig) {
  const [state, setState] = useState<State>({
    nodes: new Map(),
    edges: [],
    loading: true,
    error: null,
    stats: { node_count: 0, edge_count: 0 },
  });

  const dirtyRef = useRef<Set<string>>(new Set());
  const flushTimerRef = useRef<number | null>(null);
  const nodesRef = useRef<Map<string, MemNode>>(new Map());

  useEffect(() => {
    const ctrl = new AbortController();

    fetchGraphFull(api, ctrl.signal)
      .then((resp) => {
        if (ctrl.signal.aborted) return;
        const map = new Map<string, MemNode>();
        resp.nodes.forEach((n) => map.set(n.id, n));

        const unpositioned = resp.nodes.filter(
          (n) => n.graph_x == null || n.graph_y == null,
        );

        if (unpositioned.length > 0) {
          const patch: Array<{ id: string; x: number; y: number }> = [];

          if (unpositioned.length === resp.nodes.length) {
            // All new — full force layout
            const positions = forceLayout({ nodes: resp.nodes, edges: resp.edges });
            for (const [id, p] of Object.entries(positions)) {
              const cur = map.get(id);
              if (cur) {
                map.set(id, { ...cur, graph_x: p.x, graph_y: p.y });
                patch.push({ id, x: p.x, y: p.y });
              }
            }
          } else {
            // Some new — scatter near center of existing nodes
            const positioned = resp.nodes.filter(
              (n) => n.graph_x != null && n.graph_y != null,
            );
            let cx = 0, cy = 0;
            for (const n of positioned) { cx += n.graph_x!; cy += n.graph_y!; }
            if (positioned.length > 0) { cx /= positioned.length; cy /= positioned.length; }
            const radius = Math.max(200, positioned.length * 15);
            for (const n of unpositioned) {
              const angle = Math.random() * Math.PI * 2;
              const r = radius * (0.5 + Math.random() * 0.5);
              const x = cx + Math.cos(angle) * r;
              const y = cy + Math.sin(angle) * r;
              map.set(n.id, { ...n, graph_x: x, graph_y: y });
              patch.push({ id: n.id, x, y });
            }
          }

          if (patch.length > 0) {
            patchPositions(api, patch).catch(() => {});
          }
        }

        nodesRef.current = map;
        setState({
          nodes: map,
          edges: resp.edges,
          loading: false,
          error: null,
          stats: resp.stats,
        });
      })
      .catch((e) => {
        if (e?.name === "AbortError") return;
        setState((s) => ({ ...s, loading: false, error: String(e) }));
      });

    return () => ctrl.abort();
  }, [api]);

  const flushDirty = useCallback(() => {
    if (dirtyRef.current.size === 0) return;
    const positions: Array<{ id: string; x: number; y: number }> = [];
    for (const id of dirtyRef.current) {
      const n = nodesRef.current.get(id);
      if (n && n.graph_x != null && n.graph_y != null) {
        positions.push({ id, x: n.graph_x, y: n.graph_y });
      }
    }
    dirtyRef.current.clear();
    if (positions.length > 0) {
      patchPositions(api, positions).catch(() => {});
    }
  }, [api]);

  const updateNode = useCallback(
    (id: string, patch: Partial<MemNode>, markDirty = false) => {
      setState((prev) => {
        const next = new Map(prev.nodes);
        const cur = next.get(id);
        if (!cur) return prev;
        next.set(id, { ...cur, ...patch });
        nodesRef.current = next;
        return { ...prev, nodes: next };
      });
      if (markDirty) {
        dirtyRef.current.add(id);
        if (flushTimerRef.current != null) {
          window.clearTimeout(flushTimerRef.current);
        }
        flushTimerRef.current = window.setTimeout(flushDirty, 500);
      }
    },
    [flushDirty],
  );

  useEffect(() => {
    return () => {
      if (dirtyRef.current.size > 0) {
        const positions: Array<{ id: string; x: number; y: number }> = [];
        for (const id of dirtyRef.current) {
          const n = nodesRef.current.get(id);
          if (n && n.graph_x != null && n.graph_y != null) {
            positions.push({ id, x: n.graph_x, y: n.graph_y });
          }
        }
        dirtyRef.current.clear();
        if (positions.length > 0) {
          patchPositions(api, positions).catch(() => {});
        }
      }
      if (flushTimerRef.current != null) {
        window.clearTimeout(flushTimerRef.current);
      }
    };
  }, [api]);

  const bulkUpdatePositions = useCallback(
    (updates: Array<{ id: string; x: number; y: number }>, markDirty = false) => {
      if (updates.length === 0) return;
      setState((prev) => {
        const next = new Map(prev.nodes);
        for (const u of updates) {
          const cur = next.get(u.id);
          if (!cur) continue;
          next.set(u.id, { ...cur, graph_x: u.x, graph_y: u.y });
        }
        nodesRef.current = next;
        return { ...prev, nodes: next };
      });
      if (markDirty) {
        for (const u of updates) dirtyRef.current.add(u.id);
        if (flushTimerRef.current != null) {
          window.clearTimeout(flushTimerRef.current);
        }
        flushTimerRef.current = window.setTimeout(flushDirty, 500);
      }
    },
    [flushDirty],
  );

  const removeNode = useCallback((id: string) => {
    setState((prev) => {
      if (!prev.nodes.has(id)) return prev;
      const next = new Map(prev.nodes);
      next.delete(id);
      nodesRef.current = next;
      const edges = prev.edges.filter((e) => e.from !== id && e.to !== id);
      return {
        ...prev,
        nodes: next,
        edges,
        stats: {
          node_count: Math.max(0, prev.stats.node_count - 1),
          edge_count: edges.length,
        },
      };
    });
  }, []);

  // Event-driven refresh: listen for mem-activity write events → re-fetch graph.
  // Debounces rapid writes (e.g. auto-extraction spawning multiple saves).
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
            const next = new Map(prev.nodes);
            let changed = false;
            const patch: Array<{ id: string; x: number; y: number }> = [];

            const newNodes: MemNode[] = [];
            for (const n of resp.nodes) {
              if (!next.has(n.id)) {
                next.set(n.id, n);
                newNodes.push(n);
                changed = true;
              }
            }

            // Position new nodes via force layout — existing nodes are pinned
            const unpositioned = newNodes.filter(n => n.graph_x == null || n.graph_y == null);
            if (unpositioned.length > 0) {
              const pinned = new Set<string>();
              const allForLayout: MemNode[] = [];
              for (const [id, n] of next) {
                if (!unpositioned.some(u => u.id === id) && n.graph_x != null) {
                  pinned.add(id);
                }
                allForLayout.push(n);
              }
              const layoutResult = forceLayout({
                nodes: allForLayout,
                edges: resp.edges,
                iterations: 100,
                pinned,
              });
              for (const n of unpositioned) {
                const p = layoutResult[n.id];
                if (p) {
                  next.set(n.id, { ...n, graph_x: p.x, graph_y: p.y });
                  patch.push({ id: n.id, x: p.x, y: p.y });
                }
              }
            }

            if (!changed) return prev;
            nodesRef.current = next;
            if (patch.length > 0) patchPositions(api, patch).catch(() => {});
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
      }).then((fn) => {
        if (!active) { fn(); return; }
        unlisten = fn;
      }).catch(() => {});
    }

    return () => {
      active = false;
      if (unlisten) unlisten();
      if (debounceTimer != null) clearTimeout(debounceTimer);
    };
  }, [api]);

  return { ...state, updateNode, bulkUpdatePositions, removeNode, flushDirty };
}
