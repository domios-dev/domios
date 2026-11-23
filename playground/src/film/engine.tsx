import { createContext, useContext, type CSSProperties, type ReactNode } from "react";

export interface Scene {
  id: string;
  duration: number;
  render: (t: number) => ReactNode;
}

export interface Key {
  t: number;
  x?: number;
  y?: number;
  scale?: number;
  opacity?: number;
}

const TimeContext = createContext(0);

export const useT = () => useContext(TimeContext);

export function TimeProvider({ t, children }: { t: number; children: ReactNode }) {
  return <TimeContext.Provider value={t}>{children}</TimeContext.Provider>;
}

export interface Stage {
  width: number;
  height: number;
  portrait: boolean;
  /** Device pixels per stage pixel as the stage is shown right now; canvases size their backing store by it. */
  density: number;
}

const screenDensity = () => (typeof window !== "undefined" && window.devicePixelRatio > 0 ? window.devicePixelRatio : 1);
const StageContext = createContext<Stage>({ width: 1920, height: 1080, portrait: false, density: 1 });

/** Output frame size; scenes branch on `portrait` for their 9:16 composition. */
export const useStage = () => useContext(StageContext);

export function StageProvider({ width, height, density, children }: { width: number; height: number; density?: number; children: ReactNode }) {
  return <StageContext.Provider value={{ width, height, portrait: height > width, density: density ?? screenDensity() }}>{children}</StageContext.Provider>;
}

export const clamp = (v: number, lo = 0, hi = 1) => Math.min(hi, Math.max(lo, v));
export const lerp = (a: number, b: number, k: number) => a + (b - a) * k;
export const easeInOut = (k: number) => (k < 0.5 ? 4 * k * k * k : 1 - (-2 * k + 2) ** 3 / 2);
export const easeOut = (k: number) => 1 - (1 - k) ** 3;

/** Eased 0..1 progress of t across [a, b]. */
export const range = (t: number, a: number, b: number, ease = easeInOut) => ease(clamp((t - a) / (b - a)));

/** Interpolates keyframes by t; missing fields hold the previous key's value. */
export function track(t: number, keys: Key[], ease = easeInOut): Required<Omit<Key, "t">> {
  const filled: Required<Key>[] = [];
  let prev: Required<Key> = { t: 0, x: 0, y: 0, scale: 1, opacity: 1 };
  for (const k of keys) filled.push((prev = { ...prev, ...k }));
  const i = filled.findIndex((k) => k.t > t);
  if (i <= 0) {
    const { t: _, ...v } = filled[i === 0 ? 0 : filled.length - 1];
    return v;
  }
  const a = filled[i - 1];
  const b = filled[i];
  const k = ease(clamp((t - a.t) / (b.t - a.t)));
  return { x: lerp(a.x, b.x, k), y: lerp(a.y, b.y, k), scale: lerp(a.scale, b.scale, k), opacity: lerp(a.opacity, b.opacity, k) };
}

/** Pans and zooms a large world under a fixed viewport; (x, y) is the world point at viewport centre. */
export function Camera({ t, keys, width, height, children, style }: { t: number; keys: Key[]; width: number; height: number; children: ReactNode; style?: CSSProperties }) {
  const c = track(t, keys);
  return (
    <div style={{ position: "absolute", inset: 0, overflow: "hidden", ...style }}>
      <div
        style={{
          position: "absolute",
          left: "50%",
          top: "50%",
          width,
          height,
          transformOrigin: "0 0",
          transform: `scale(${c.scale}) translate(${-c.x}px, ${-c.y}px)`,
          opacity: c.opacity,
        }}
      >
        {children}
      </div>
    </div>
  );
}

export interface FocusKey {
  t: number;
  /** Lit rectangle in stage pixels; omit to light the whole frame. */
  rect?: { x: number; y: number; w: number; h: number };
  dim?: number;
  label?: string;
}

/** Dims everything outside a moving lit rectangle so attention travels between details; place above the scene content. */
export function Spotlight({ t, keys, radius = 18 }: { t: number; keys: FocusKey[]; radius?: number }) {
  const i = Math.max(0, keys.findIndex((k) => k.t > t) - 1);
  const a = keys.findIndex((k) => k.t > t) === -1 ? keys[keys.length - 1] : keys[i];
  const b = keys.findIndex((k) => k.t > t) === -1 ? a : keys[i + 1] ?? a;
  const k = a === b ? 1 : range(t, a.t, b.t);
  const full = { x: -40, y: -40, w: 4000, h: 4000 };
  const ra = a.rect ?? full;
  const rb = b.rect ?? full;
  const r = { x: lerp(ra.x, rb.x, k), y: lerp(ra.y, rb.y, k), w: lerp(ra.w, rb.w, k), h: lerp(ra.h, rb.h, k) };
  const dim = lerp(a.rect ? a.dim ?? 0.62 : 0, b.rect ? b.dim ?? 0.62 : 0, k);
  const label = k < 0.5 ? a.label : b.label;
  const labelOpacity = Math.abs(k - 0.5) * 2;
  if (dim <= 0.001) return null;
  return (
    <div style={{ position: "absolute", inset: 0, pointerEvents: "none", zIndex: 50 }}>
      <div
        style={{
          position: "absolute",
          left: r.x,
          top: r.y,
          width: r.w,
          height: r.h,
          borderRadius: radius,
          boxShadow: `0 0 0 4000px color-mix(in srgb, var(--ct-bg) ${Math.round(dim * 100)}%, transparent), 0 0 0 1.5px color-mix(in srgb, var(--ct-accent) ${Math.round(dim * 70)}%, transparent)`,
        }}
      />
      {label && (
        <div
          style={{
            position: "absolute",
            left: r.x,
            top: r.y + r.h + 14,
            opacity: labelOpacity,
            color: "var(--ct-fg)",
            fontSize: 26,
            fontWeight: 600,
            letterSpacing: "-0.01em",
          }}
        >
          {label}
        </div>
      )}
    </div>
  );
}

/** Fades and lifts content in at `at` over `dur` seconds. */
export function Reveal({ t, at, dur = 0.5, dy = 16, children, style }: { t: number; at: number; dur?: number; dy?: number; children: ReactNode; style?: CSSProperties }) {
  const k = range(t, at, at + dur, easeOut);
  return <div style={{ opacity: k, transform: `translateY(${(1 - k) * dy}px)`, ...style }}>{children}</div>;
}

/** Hard-cut sequence: scenes play back to back with no transitions. */
export function sequence(id: string, scenes: Scene[]): Scene {
  const starts = scenes.map((_, i) => scenes.slice(0, i).reduce((s, x) => s + x.duration, 0));
  const duration = scenes.reduce((s, x) => s + x.duration, 0);
  return {
    id,
    duration,
    render: (t) => {
      let i = starts.findIndex((s, j) => t >= s && t < s + scenes[j].duration);
      if (i < 0) i = scenes.length - 1;
      return <TimeProvider t={t - starts[i]}>{scenes[i].render(t - starts[i])}</TimeProvider>;
    },
  };
}
