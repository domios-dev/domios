import { useCallback, useEffect, useRef, useState } from "react";
import Graph from "graphology";
import type { MemEdge, MemNode } from "./types";
import type {
  PhysicsNode,
  PhysicsRequest,
  PhysicsResponse,
} from "./physicsProtocol";
import { seedPosition } from "./seedLayout";
import { layoutLouvain } from "./louvainLayout";

export interface Position {
  x: number;
  y: number;
}

interface Opts {
  nodes: Map<string, MemNode>;
  edges: MemEdge[];
  enabled: boolean;
}

export interface GraphLayout {
  positions: Map<string, Position>;
  version: number;
  heat: () => void;
  setPosition: (id: string, pos: Position) => void;
}

export function useGraphLayout({ nodes, edges, enabled }: Opts): GraphLayout {
  const workerRef = useRef<Worker | null>(null);
  const positionsRef = useRef<Map<string, Position>>(new Map());
  const seededRef = useRef(false);
  const [version, setVersion] = useState(0);
  const bumpScheduledRef = useRef(false);

  const post = useCallback((msg: PhysicsRequest) => {
    workerRef.current?.postMessage(msg);
  }, []);

  const scheduleBump = useCallback(() => {
    if (bumpScheduledRef.current) return;
    bumpScheduledRef.current = true;
    requestAnimationFrame(() => {
      bumpScheduledRef.current = false;
      setVersion((v) => (v + 1) & 0x7fffffff);
    });
  }, []);

  useEffect(() => {
    const worker = new Worker(
      new URL("./physics.worker.ts", import.meta.url),
      { type: "module" },
    );
    workerRef.current = worker;
    worker.onmessage = (ev: MessageEvent<PhysicsResponse>) => {
      const msg = ev.data;
      if (msg.type === "tick") {
        for (const p of msg.positions) {
          positionsRef.current.set(p.id, { x: p.x, y: p.y });
        }
        scheduleBump();
      }
    };
    return () => {
      worker.terminate();
      workerRef.current = null;
    };
  }, [scheduleBump]);

  useEffect(() => {
    post({ type: "setEnabled", enabled });
  }, [enabled, post]);

  useEffect(() => {
    const positions = positionsRef.current;

    if (!seededRef.current && nodes.size > 0) {
      seededRef.current = true;
      const seedGraph = new Graph({ multi: false });
      for (const id of nodes.keys()) seedGraph.addNode(id);
      for (const e of edges) {
        if (!seedGraph.hasNode(e.from) || !seedGraph.hasNode(e.to)) continue;
        const key = `${e.from}→${e.to}`;
        if (!seedGraph.hasEdge(key)) {
          try {
            seedGraph.addEdgeWithKey(key, e.from, e.to, {
              weight: e.confidence ?? 0.5,
            });
          } catch {
            /* duplicate — ignore */
          }
        }
      }
      layoutLouvain(seedGraph);
      seedGraph.forEachNode((id, attrs) => {
        const x = attrs.x as number | undefined;
        const y = attrs.y as number | undefined;
        if (typeof x === "number" && typeof y === "number" &&
            Number.isFinite(x) && Number.isFinite(y)) {
          positions.set(id, { x, y });
        }
      });
      scheduleBump();
    }

    const snapshot: PhysicsNode[] = [];
    const count = nodes.size;
    let dirty = false;
    for (const id of nodes.keys()) {
      let pos = positions.get(id);
      if (!pos) {
        pos = seedPosition(id, count);
        positions.set(id, pos);
        dirty = true;
      }
      snapshot.push({ id, x: pos.x, y: pos.y });
    }
    for (const id of [...positions.keys()]) {
      if (!nodes.has(id)) {
        positions.delete(id);
        dirty = true;
      }
    }
    if (dirty) scheduleBump();

    post({
      type: "setGraph",
      nodes: snapshot,
      edges: edges.map((e) => ({
        from: e.from,
        to: e.to,
        confidence: e.confidence,
      })),
    });
  }, [nodes, edges, post, scheduleBump]);

  const heat = useCallback(() => post({ type: "heat" }), [post]);

  const setPosition = useCallback(
    (id: string, pos: Position) => {
      positionsRef.current.set(id, pos);
      post({ type: "updatePosition", id, x: pos.x, y: pos.y });
      scheduleBump();
    },
    [post, scheduleBump],
  );

  return {
    positions: positionsRef.current,
    version,
    heat,
    setPosition,
  };
}
