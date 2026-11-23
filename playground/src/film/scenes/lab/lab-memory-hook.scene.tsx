import { useState } from "react";
import topology from "../../data/memory-topology.json";
import { easeOut, lerp, range, useStage, type Scene } from "../../engine";
import { HoloCanvas } from "../mesh/holoMemory";
import { memoryColor, type HoloCast, type HoloState } from "../mesh/holoTheme";
import { centreOf, placeOf } from "../mesh/holoVolume";
import { fmt, NODES, StageCanvas, type GraphLayer, type GraphNode } from "./labGraph";

// The film's first seconds. One memory glows alone in the dark; on "This" it bursts into the whole year of memory, a
// volume that flies past the edges of the player and turns so it reads as 3D at once. On "Thousands of facts, all
// connected" a pulse of light runs through every relation from that memory, and the camera starts a steady approach to
// it. lab-memory-origin picks the volume up from here and gathers it back into that one memory.

const DURATION = 7;
/** lab-memory-origin's first fact: the burst leaves it, the camera closes in on it and the volume gathers back into it. */
export const ORIGIN = 221;
const BAM = 0.35;
const WAVE = 4.45;
const APPROACH = [4.9, DURATION] as const;
const START_SCALE = 2.4;
const linear = (k: number) => k;
const noop = () => undefined;

type View = { x: number; y: number; scale: number };
export interface HoloFrame {
  width: number;
  height: number;
  nodes: GraphNode[];
  /** Where lab-memory-origin's beacon sits: the gathered memory lands there. */
  fx: number;
  fy: number;
  /** The volume's framing: world point at the centre and scale, big enough that it runs past the frame. */
  wide: View;
  title: { left: number; top: number; size: number; stats: number };
}

const pt = ([x, y]: [number, number]) => ({ x, y });
/** Like memory-graph, the tall frame turns the layout a quarter to fill 9:16. */
const PORTRAIT_NODES = NODES.map(([x, y, k, d]): GraphNode => [-y, x, k, d]);
const LANDSCAPE: HoloFrame = { width: 1920, height: 1080, nodes: NODES, fx: 960, fy: 540, wide: { ...pt(centreOf(NODES)), scale: 0.8 }, title: { left: 72, top: 64, size: 30, stats: 21 } };
const PORTRAIT: HoloFrame = { width: 1080, height: 1920, nodes: PORTRAIT_NODES, fx: 540, fy: 768, wide: { ...pt(centreOf(PORTRAIT_NODES)), scale: 0.86 }, title: { left: 64, top: 110, size: 40, stats: 30 } };
export const holoFrame = (portrait: boolean) => (portrait ? PORTRAIT : LANDSCAPE);

export const HOOK_CAST: HoloCast = { ego: ORIGIN, fresh: -1 };

/** The turn: a wide swing out of the burst so the depth reads at once, then a steady drift that never stops. */
export function hookTurn(t: number): { yaw: number; pitch: number } {
  return {
    yaw: -0.95 + 1.25 * range(t, BAM, BAM + 3.2, easeOut) + 0.07 * Math.max(0, t - (BAM + 3.2)),
    pitch: 0.5 - 0.22 * range(t, BAM, BAM + 3.2, easeOut),
  };
}

/** Pulled back from the memory to the whole volume, then a steady approach to the memory where it is now in 3D. */
function camera(t: number, f: HoloFrame): View {
  const pull = range(t, BAM, BAM + 2.3, easeOut);
  // Eased in only at its start, then constant: the camera flies at the memory the whole way rather than catching up.
  const a = range(t, APPROACH[0], APPROACH[1], (k) => (k < 0.2 ? (k * k) / 0.4 : k - 0.1) / 0.9);
  const [ox, oy] = f.nodes[ORIGIN];
  const look = { x: lerp(ox, f.wide.x, pull), y: lerp(oy, f.wide.y, pull) };
  const { yaw, pitch } = hookTurn(t);
  // The volume is only built once it is needed: never for the poster, which sits before the approach.
  const at = a > 0 ? placeOf(ORIGIN, f.nodes, 0, yaw, pitch) : look;
  const scale = Math.exp(lerp(lerp(Math.log(START_SCALE), Math.log(f.wide.scale), pull), Math.log(f.wide.scale * 1.8), a));
  return { x: lerp(look.x, at.x, a), y: lerp(look.y, at.y, a), scale };
}

export function hookState(t: number, f: HoloFrame): HoloState {
  const { yaw, pitch } = hookTurn(t);
  const ring = (t - BAM) / 1.2;
  const wave = t - WAVE;
  return {
    view: camera(t, f),
    burst: t - BAM,
    flatten: 0,
    holo: 1,
    yaw,
    pitch,
    ego: 0,
    scan: null,
    ring: ring >= 0 && ring < 1 ? ring : null,
    opacity: t < BAM ? 0 : 1,
    collapse: 1,
    from: f.nodes[ORIGIN],
    flight: 0.75,
    overshoot: 1.6,
    stagger: 1.7,
    wave: wave >= 0 && wave < 2.2 ? wave : null,
  };
}

/** lab-memory-origin's first frame continues this one. */
export const hookEnd = (portrait: boolean) => hookState(DURATION, holoFrame(portrait));

function MemoryHook({ t }: { t: number }) {
  const { portrait } = useStage();
  const f = holoFrame(portrait);
  const [failed, setFailed] = useState(false);
  const orange = memoryColor();
  const state = hookState(t, f);
  const view = state.view;
  // Only without WebGL does the flat graph carry the cut. While the hologram's chunk is still arriving nothing stands in:
  // a stand-in would build its own WebGL context (and spill) only to drop it a moment later, the start-up peak that can
  // get a phone tab reloaded.
  const flat: GraphLayer = { view: { cx: view.x, cy: view.y, scale: view.scale, fx: f.width / 2, fy: f.height / 2 }, positions: f.nodes, opacity: failed && t >= BAM ? 1 : 0 };

  // The lone memory before the burst.
  const seed = 1 - range(t, BAM - 0.02, BAM + 0.12, linear);
  const pulse = 0.5 + 0.5 * Math.sin(t * 14);
  const title = range(t, BAM + 0.8, BAM + 1.4, easeOut) * (1 - range(t, APPROACH[0] + 0.6, APPROACH[0] + 1.1));

  return (
    <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)" }}>
      {/* Mounted only while it shows: every canvas here holds a WebGL context, and phones have few to spare. */}
      {(flat.opacity ?? 1) > 0 && <StageCanvas t={t} layers={[flat]} />}
      <HoloCanvas t={t} state={state} layout={f.nodes} cast={HOOK_CAST} onReady={noop} onFailed={() => setFailed(true)} />
      {seed > 0 && (
        <svg style={{ position: "absolute", inset: 0, width: "100%", height: "100%" }} aria-hidden="true">
          <circle cx={f.width / 2} cy={f.height / 2} r={26 + 10 * pulse} fill={orange} opacity={0.18 * seed} />
          <circle cx={f.width / 2} cy={f.height / 2} r={9 + 2 * pulse} fill={orange} opacity={seed} />
        </svg>
      )}
      {title > 0 && (
        <div style={{ position: "absolute", left: f.title.left, top: f.title.top, opacity: title, transform: `translateY(${(1 - title) * 10}px)` }}>
          <div style={{ fontSize: f.title.size, fontWeight: 600, color: "var(--ct-fg)", letterSpacing: "-0.01em" }}>mac-mini · memory, a year in</div>
          <div style={{ marginTop: 10, fontSize: f.title.stats, color: "var(--ct-muted-fg)", fontVariantNumeric: "tabular-nums" }}>
            {fmt(topology.totals.nodes)} nodes · {fmt(topology.totals.edges)} relations
          </div>
        </div>
      )}
    </div>
  );
}

const memoryHook: Scene = {
  id: "lab-memory-hook",
  duration: DURATION,
  render: (t) => <MemoryHook t={t} />,
};

export default memoryHook;
