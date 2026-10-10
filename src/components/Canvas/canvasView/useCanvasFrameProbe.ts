import { useEffect, useRef } from "react";
import type { ViewportTransform } from "../useCanvasViewport";
import { DEV_BUILD } from "../../../lib/devBuild";


export interface FrameProbeStats {
  label: string;
  frames: number;
  p50: number;
  p95: number;
  max: number;
  droppedPct: number;
}

function isDevBuild(): boolean {
  try {
    return DEV_BUILD;
  } catch {
    return false;
  }
}

const FRAME_BUDGET_MS = 1000 / 60;
const DROP_THRESHOLD_MS = FRAME_BUDGET_MS * 1.5; // a frame >25ms = a visible drop

function percentile(sorted: number[], p: number): number {
  if (!sorted.length) return 0;
  const i = Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length));
  return sorted[i];
}

function summarize(label: string, deltas: number[]): FrameProbeStats {
  const sorted = [...deltas].sort((a, b) => a - b);
  const dropped = deltas.filter((d) => d > DROP_THRESHOLD_MS).length;
  const r1 = (n: number) => Math.round(n * 10) / 10;
  return {
    label,
    frames: deltas.length,
    p50: r1(percentile(sorted, 50)),
    p95: r1(percentile(sorted, 95)),
    max: r1(sorted[sorted.length - 1] ?? 0),
    droppedPct: deltas.length ? r1((dropped / deltas.length) * 100) : 0,
  };
}

function pushPerf(stats: FrameProbeStats): void {
  const w = window as unknown as { __ctPerf?: { cmd: string; ms: number; at: number }[] };
  (w.__ctPerf ??= []).push({ cmd: `CANVAS:${stats.label} p95`, ms: stats.p95, at: Date.now() });
  if (w.__ctPerf.length > 200) w.__ctPerf.shift();
}

class FrameSampler {
  private deltas: number[] = [];
  private raf: number | null = null;
  private last = 0;
  private label = "gesture";

  start(label: string): void {
    this.label = label;
    this.deltas = [];
    this.last = 0;
    if (this.raf != null) return;
    const loop = (ts: number) => {
      if (this.last) this.deltas.push(ts - this.last);
      this.last = ts;
      this.raf = requestAnimationFrame(loop);
    };
    this.raf = requestAnimationFrame(loop);
  }

  stop(): FrameProbeStats | null {
    if (this.raf == null) return null;
    cancelAnimationFrame(this.raf);
    this.raf = null;
    const stats = summarize(this.label, this.deltas);
    this.deltas = [];
    this.last = 0;
    if (stats.frames > 2) {
      // eslint-disable-next-line no-console
      console.info(`[canvas-perf] ${stats.label}: p50=${stats.p50}ms p95=${stats.p95}ms max=${stats.max}ms dropped=${stats.droppedPct}% (${stats.frames}f)`);
      pushPerf(stats);
    }
    return stats;
  }
}

export const canvasFrameSampler = new FrameSampler();

const IDLE_MS = 220; // gesture considered over after this quiet gap (matches commit throttle tail)

export function useCanvasFrameProbe(transform: ViewportTransform): void {
  const idleRef = useRef<number | null>(null);
  const activeRef = useRef(false);

  useEffect(() => {
    if (!isDevBuild()) return;
    if (!activeRef.current) {
      activeRef.current = true;
      canvasFrameSampler.start("gesture");
    }
    if (idleRef.current != null) clearTimeout(idleRef.current);
    idleRef.current = window.setTimeout(() => {
      activeRef.current = false;
      idleRef.current = null;
      canvasFrameSampler.stop();
    }, IDLE_MS);
  }, [transform]);

  useEffect(() => {
    if (!isDevBuild()) return;
    const w = window as unknown as { __ctCanvasProbe?: unknown };
    w.__ctCanvasProbe = {
      start: (label = "manual") => canvasFrameSampler.start(label),
      stop: () => canvasFrameSampler.stop(),
    };
    return () => {
      if (idleRef.current != null) clearTimeout(idleRef.current);
    };
  }, []);
}
