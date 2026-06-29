import { useCallback, useEffect, useRef } from "react";
import type { PresenceOverlay } from "./presenceOverlays";

// EMERGENCE: when an agent's task assignment changes, its presence pin eases from
// the old task corner to the new one. Offsets are WORLD-space inside the camera
// layer; the glide wrapper transform is written imperatively only.

const GLIDE_DURATION_MS = 520;

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

function easeOutCubic(t: number): number {
  return 1 - Math.pow(1 - t, 3);
}

interface AgentGlideState {
  /** Task the agent currently rests on — its base (world) anchor. */
  taskId: string;
  anchor: { x: number; y: number };
  /** Active glide offset (world px) decaying to 0; null = resting at the anchor. */
  glide: { startTs: number | null; ox: number; oy: number } | null;
}

export class PresenceGlideController {
  private readonly agents = new Map<string, AgentGlideState>();
  private readonly elByTask = new Map<string, HTMLElement>();
  private raf: number | null = null;

  registerPin(taskId: string, el: HTMLElement | null): void {
    if (el) this.elByTask.set(taskId, el);
    else this.elByTask.delete(taskId);
  }

  // Reconcile against the current (budgeted, viewport-culled) overlay set: agents
  // that changed task start/retarget a glide; agents that vanished are dropped so
  // the tracked set never exceeds what's on screen.
  sync(overlays: readonly PresenceOverlay[]): void {
    const reduce = prefersReducedMotion();
    const seen = new Set<string>();
    for (const o of overlays) {
      const lead = o.agents[0];
      if (!lead) continue;
      seen.add(lead.id);
      const home = { x: o.anchor.x, y: o.anchor.y };
      const prev = this.agents.get(lead.id);
      if (!prev) {
        this.agents.set(lead.id, { taskId: o.taskId, anchor: home, glide: null });
        continue;
      }
      if (prev.taskId === o.taskId) {
        prev.anchor = home; // same task: base follows the camera; keep any glide.
        continue;
      }
      // Task changed → glide from the agent's CURRENT visual position to the new
      // corner (retarget-safe: continues from wherever a running glide is now).
      const cur = this.offsetNow(prev);
      const ox = prev.anchor.x + cur.x - home.x;
      const oy = prev.anchor.y + cur.y - home.y;
      prev.taskId = o.taskId;
      prev.anchor = home;
      if (reduce || (Math.abs(ox) < 0.5 && Math.abs(oy) < 0.5)) {
        prev.glide = null;
        this.writeEl(o.taskId, 0, 0);
      } else {
        prev.glide = { startTs: null, ox, oy };
        this.writeEl(o.taskId, ox, oy); // paint the start pose now (no destination flash).
      }
    }
    for (const id of [...this.agents.keys()]) if (!seen.has(id)) this.agents.delete(id);
    if (this.hasActive()) this.ensureLoop();
  }

  dispose(): void {
    if (this.raf != null) cancelAnimationFrame(this.raf);
    this.raf = null;
    this.agents.clear();
    this.elByTask.clear();
  }

  private offsetNow(s: AgentGlideState): { x: number; y: number } {
    const g = s.glide;
    if (!g) return { x: 0, y: 0 };
    if (g.startTs == null) return { x: g.ox, y: g.oy };
    const t = Math.min(1, (performance.now() - g.startTs) / GLIDE_DURATION_MS);
    const e = easeOutCubic(t);
    return { x: g.ox * (1 - e), y: g.oy * (1 - e) };
  }

  private hasActive(): boolean {
    for (const s of this.agents.values()) if (s.glide) return true;
    return false;
  }

  private ensureLoop(): void {
    if (this.raf == null) this.raf = requestAnimationFrame(this.tick);
  }

  private readonly tick = (ts: number): void => {
    this.raf = null;
    let active = false;
    for (const s of this.agents.values()) {
      const g = s.glide;
      if (!g) continue;
      if (g.startTs == null) g.startTs = ts;
      const t = Math.min(1, (ts - g.startTs) / GLIDE_DURATION_MS);
      const e = easeOutCubic(t);
      this.writeEl(s.taskId, g.ox * (1 - e), g.oy * (1 - e));
      if (t < 1) active = true;
      else {
        s.glide = null;
        this.writeEl(s.taskId, 0, 0);
      }
    }
    if (active) this.ensureLoop();
  };

  private writeEl(taskId: string, x: number, y: number): void {
    const el = this.elByTask.get(taskId);
    if (el) el.style.transform = x === 0 && y === 0 ? "" : `translate(${x}px, ${y}px)`;
  }
}

export interface PresenceGlideApi {
  /** Stable per-task ref callback for the pin's glide wrapper. */
  pinRef: (taskId: string) => (el: HTMLElement | null) => void;
}

// Owns a controller for the layer's lifetime, syncs it after every overlay commit,
// and hands out stable ref callbacks so the wrapper element isn't detached/reattached
// (which would wipe an in-flight glide) on camera-coupled re-renders.
export function usePresenceGlide(overlays: readonly PresenceOverlay[]): PresenceGlideApi {
  const ctrlRef = useRef<PresenceGlideController | null>(null);
  if (!ctrlRef.current) ctrlRef.current = new PresenceGlideController();
  const ctrl = ctrlRef.current;
  const refCache = useRef(new Map<string, (el: HTMLElement | null) => void>());

  useEffect(() => () => ctrl.dispose(), [ctrl]);
  useEffect(() => {
    ctrl.sync(overlays);
  }, [ctrl, overlays]);

  const pinRef = useCallback(
    (taskId: string) => {
      const cache = refCache.current;
      let cb = cache.get(taskId);
      if (!cb) {
        cb = (el: HTMLElement | null) => ctrl.registerPin(taskId, el);
        cache.set(taskId, cb);
      }
      return cb;
    },
    [ctrl],
  );

  return { pinRef };
}
