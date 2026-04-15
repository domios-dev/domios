import { useCallback, useEffect, useRef, useState } from "react";
import type { MemEdge, MemNode } from "./types";
import type {
  PhysicsNode,
  PhysicsRequest,
  PhysicsResponse,
} from "./physicsProtocol";
import { seedPosition } from "./seedLayout";

export interface Position {
  x: number;
  y: number;
}

interface Opts {
  nodes: Map<string, MemNode>;
  edges: MemEdge[];
  pinnedPositions: Map<string, Position>;
  enabled: boolean;
}

export interface GraphLayout {
  positions: Map<string, Position>;
  version: number;
  heat: () => void;
  pin: (id: string, pos: Position) => void;
  unpin: (id: string) => void;
  setPosition: (id: string, pos: Position) => void;
}

export function useGraphLayout({
  nodes,
  edges,
  pinnedPositions,
  enabled,
}: Opts): GraphLayout {
  const workerRef = useRef<Worker | null>(null);
  const positionsRef = useRef<Map<string, Position>>(new Map());
  const pinnedRef = useRef<Set<string>>(new Set());
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
    const pinnedSet = pinnedRef.current;
    pinnedSet.clear();
    for (const id of pinnedPositions.keys()) pinnedSet.add(id);

    const snapshot: PhysicsNode[] = [];
    const count = nodes.size;
    let dirty = false;
    for (const id of nodes.keys()) {
      const pin = pinnedPositions.get(id);
      let pos = positions.get(id);
      if (pin) {
        positions.set(id, pin);
        pos = pin;
      } else if (!pos) {
        pos = seedPosition(id, count);
        positions.set(id, pos);
        dirty = true;
      }
      snapshot.push({
        id,
        x: pos.x,
        y: pos.y,
        pinned: !!pin,
      });
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
  }, [nodes, edges, pinnedPositions, post, scheduleBump]);

  const heat = useCallback(() => post({ type: "heat" }), [post]);

  const pin = useCallback(
    (id: string, pos: Position) => {
      pinnedRef.current.add(id);
      positionsRef.current.set(id, pos);
      post({ type: "updatePosition", id, x: pos.x, y: pos.y });
      post({ type: "setPinned", pinned: [...pinnedRef.current] });
      scheduleBump();
    },
    [post, scheduleBump],
  );

  const unpin = useCallback(
    (id: string) => {
      pinnedRef.current.delete(id);
      post({ type: "setPinned", pinned: [...pinnedRef.current] });
      post({ type: "heat" });
    },
    [post],
  );

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
    pin,
    unpin,
    setPosition,
  };
}
