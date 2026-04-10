import { useCallback, useEffect, useRef } from "react";
import type { MemEdge, MemNode } from "./types";

interface PhysicsOpts {
  enabled: boolean;
  nodes: Map<string, MemNode>;
  edges: MemEdge[];
  pinnedRef: React.MutableRefObject<Set<string>>;
  onTick: (updates: Array<{ id: string; x: number; y: number }>) => void;
}

interface Vec {
  vx: number;
  vy: number;
}

const REPULSION = 18000;
const SPRING_LENGTH = 250;
const SPRING_K = 0.015;
const CENTER_PULL = 0.0003;
const DAMPING = 0.78;
const MAX_VEL = 35;
const ENERGY_SLEEP = 0.05;

export function usePhysics({ enabled, nodes, edges, pinnedRef, onTick }: PhysicsOpts) {
  const velRef = useRef<Map<string, Vec>>(new Map());
  const rafRef = useRef<number | null>(null);
  const sleepingRef = useRef(false);
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const nodesRef = useRef(nodes);
  nodesRef.current = nodes;
  const edgesRef = useRef(edges);
  edgesRef.current = edges;

  const heat = useCallback(() => {
    sleepingRef.current = false;
  }, []);

  const step = useCallback(() => {
    if (!enabledRef.current) {
      rafRef.current = null;
      return;
    }
    const nodeMap = nodesRef.current;
    const edgeList = edgesRef.current;
    const pinned = pinnedRef.current;

    const entries: Array<[string, MemNode]> = [];
    for (const [id, n] of nodeMap) {
      if (n.graph_x == null || n.graph_y == null) continue;
      entries.push([id, n]);
    }
    const n = entries.length;
    if (n === 0) {
      rafRef.current = requestAnimationFrame(step);
      return;
    }

    const fx: Record<string, number> = {};
    const fy: Record<string, number> = {};
    for (const [id] of entries) {
      fx[id] = 0;
      fy[id] = 0;
    }

    // Repulsion (Coulomb-ish, O(N²) — fine below ~300 nodes)
    for (let i = 0; i < n; i++) {
      const [idA, a] = entries[i];
      const ax = a.graph_x!;
      const ay = a.graph_y!;
      for (let j = i + 1; j < n; j++) {
        const [idB, b] = entries[j];
        const dx = ax - b.graph_x!;
        const dy = ay - b.graph_y!;
        const d2 = dx * dx + dy * dy + 0.01;
        const d = Math.sqrt(d2);
        const f = REPULSION / d2;
        const nx = dx / d;
        const ny = dy / d;
        fx[idA] += nx * f;
        fy[idA] += ny * f;
        fx[idB] -= nx * f;
        fy[idB] -= ny * f;
      }
    }

    // Spring attraction along edges
    for (const e of edgeList) {
      const a = nodeMap.get(e.from);
      const b = nodeMap.get(e.to);
      if (!a || !b || a.graph_x == null || b.graph_x == null) continue;
      const dx = b.graph_x - a.graph_x;
      const dy = b.graph_y! - a.graph_y!;
      const d = Math.sqrt(dx * dx + dy * dy) + 0.01;
      const delta = d - SPRING_LENGTH;
      const f = SPRING_K * delta * (e.confidence ?? 0.7);
      const nx = dx / d;
      const ny = dy / d;
      fx[e.from] += nx * f * d * 0.02;
      fy[e.from] += ny * f * d * 0.02;
      fx[e.to] -= nx * f * d * 0.02;
      fy[e.to] -= ny * f * d * 0.02;
      // simpler spring: push along axis
      fx[e.from] += nx * f;
      fy[e.from] += ny * f;
      fx[e.to] -= nx * f;
      fy[e.to] -= ny * f;
    }

    // Gentle center pull
    for (const [id, node] of entries) {
      fx[id] -= (node.graph_x ?? 0) * CENTER_PULL;
      fy[id] -= (node.graph_y ?? 0) * CENTER_PULL;
    }

    // Integrate
    const vel = velRef.current;
    const updates: Array<{ id: string; x: number; y: number }> = [];
    let energy = 0;
    for (const [id, node] of entries) {
      if (pinned.has(id)) {
        vel.set(id, { vx: 0, vy: 0 });
        continue;
      }
      const v = vel.get(id) ?? { vx: 0, vy: 0 };
      v.vx = (v.vx + fx[id]) * DAMPING;
      v.vy = (v.vy + fy[id]) * DAMPING;
      const speed = Math.sqrt(v.vx * v.vx + v.vy * v.vy);
      if (speed > MAX_VEL) {
        v.vx = (v.vx / speed) * MAX_VEL;
        v.vy = (v.vy / speed) * MAX_VEL;
      }
      vel.set(id, v);
      energy += speed;
      const nx = (node.graph_x ?? 0) + v.vx;
      const ny = (node.graph_y ?? 0) + v.vy;
      updates.push({ id, x: nx, y: ny });
    }

    if (updates.length > 0) onTick(updates);

    if (energy / n < ENERGY_SLEEP && pinned.size === 0) {
      sleepingRef.current = true;
      rafRef.current = null;
      return;
    }
    rafRef.current = requestAnimationFrame(step);
  }, [onTick, pinnedRef]);

  useEffect(() => {
    if (!enabled) {
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
      sleepingRef.current = false;
      return;
    }
    sleepingRef.current = false;
    if (rafRef.current == null) rafRef.current = requestAnimationFrame(step);
    return () => {
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    };
  }, [enabled, step]);

  // External heater — call when user drags/adds nodes to resume
  useEffect(() => {
    if (!enabled) return;
    if (sleepingRef.current && rafRef.current == null) {
      sleepingRef.current = false;
      rafRef.current = requestAnimationFrame(step);
    }
  });

  return { heat };
}
