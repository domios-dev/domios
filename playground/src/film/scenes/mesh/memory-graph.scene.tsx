import { useState, type CSSProperties } from "react";
import { StageCanvas, type GraphLayer } from "../lab/labGraph";
import topology from "../../data/memory-topology.json";
import { easeInOut, easeOut, lerp, range, useStage, type Scene } from "../../engine";
import { HoloCanvas } from "./holoMemory";
import { themeIsLight, type HoloCast, type HoloState } from "./holoTheme";
import { H, PH, PW, W } from "./meshFixtures";

type Node = [x: number, y: number, kind: number, degree: number];
type View = { x: number; y: number; scale: number };
type Frame = { width: number; height: number; nodes: Node[]; end: View; push: View; top: number; bottom: number };

const nodes = topology.nodes as Node[];
const edges = topology.edges as Array<[number, number]>;
const totals = topology.totals;

const DURATION = 8;

const HUB = nodes.reduce((best, n, i) => (n[2] >= 0 && n[3] > nodes[best][3] ? i : best), nodes.findIndex((n) => n[2] >= 0));
const HUB_EDGES = edges.filter(([a, b]) => a === HUB || b === HUB);
const HUB_NEIGHBOURS = new Set(HUB_EDGES.flatMap(([a, b]) => [a, b]));

/**
 * The memory that arrives: a well-linked memory near the middle of the graph, so it lands inside the volume, whose links
 * reach across it, and whose every far link ends in a dense cluster, so each lit link visibly lands somewhere. Not the
 * first memory, whose links open the cut.
 */
const FRESH = (() => {
  const cx = nodes.reduce((s, n) => s + n[0], 0) / nodes.length;
  const cy = nodes.reduce((s, n) => s + n[1], 0) / nodes.length;
  const neighbours: number[][] = nodes.map(() => []);
  for (const [a, b] of edges) neighbours[a].push(b), neighbours[b].push(a);
  const crowd = (i: number) => nodes.filter((n) => Math.hypot(n[0] - nodes[i][0], n[1] - nodes[i][1]) < 45).length;
  let best = -1;
  for (let i = 0; i < nodes.length; i++) {
    const [x, y, kind, degree] = nodes[i];
    if (i === HUB || kind < 0 || degree < 10 || degree > 40) continue;
    const far = neighbours[i].filter((j) => Math.hypot(nodes[j][0] - x, nodes[j][1] - y) > 120);
    if (!far.length || far.some((j) => crowd(j) < 50)) continue;
    if (best < 0 || Math.hypot(x - cx, y - cy) < Math.hypot(nodes[best][0] - cx, nodes[best][1] - cy)) best = i;
  }
  return best;
})();
const CAST: HoloCast = { ego: FRESH, fresh: FRESH };

const frame = (width: number, height: number, list: Node[], end: View, push: View): Frame => ({
  width, height, nodes: list, end, push, top: Math.min(...list.map((n) => n[1])), bottom: Math.max(...list.map((n) => n[1])),
});

// `push` is lab-dreaming's opening view (centre [-545,-205] at its focus, same topology) expressed in this camera.
// The volume sits in the middle and a little larger than the frame, so its edges run on past the player.
const LANDSCAPE = frame(W, H, nodes, { x: 60, y: -40, scale: 0.76 }, { x: -516.4, y: -210.7, scale: 7.0 });

/** The force layout has no meaningful orientation, so the tall frame turns it a quarter to fill 9:16 instead of cropping. */
const PORTRAIT = frame(PW, PH, nodes.map(([x, y, k, d]): Node => [-y, x, k, d]), { x: 17, y: -120, scale: 0.8 }, { x: 205, y: -499.5, scale: 6.6 });

/**
 * The beat. The cut opens on lab-memory-origin's last frame, flat, the first memory's links lit. From LIFT the same graph
 * rises back into the hologram the film opened with, all at once and smoothly (the burst was the opening's), while the
 * camera pulls back and the volume turns. A new memory streaks in from past the frame and snaps its links into place:
 * "everything you teach them gets remembered, and connected to what it belongs with". In the closing push it folds back
 * into the plane and lands on lab-dreaming's opening framing, so that cut is one camera move.
 */
const BAM = 0.55;
const ARRIVE = [1.0, 2.3] as const;
const PUSH = [DURATION - 0.9, DURATION] as const;
const FOLD = [PUSH[0], PUSH[0] + 0.7] as const;
const linear = (k: number) => k;

function camera(t: number, frame: Frame): View {
  const [hx, hy] = frame.nodes[HUB];
  const { end, push } = frame;
  const pull = range(t, BAM, BAM + 1.8, easeOut);
  const drift = range(t, BAM + 1.8, PUSH[0], easeInOut);
  const scale = lerp(lerp(Math.log(3.1), Math.log(end.scale * 1.12), pull), Math.log(end.scale), drift);
  const pulled = { x: lerp(hx, end.x, pull), y: lerp(hy, end.y, pull) };
  const p = range(t, PUSH[0], PUSH[1], easeInOut);
  return { x: lerp(pulled.x, push.x, p), y: lerp(pulled.y, push.y, p), scale: Math.exp(lerp(scale, Math.log(push.scale), p)) };
}

/** The first memory's links: lit from nothing at the cut, gone once the volume has settled. */
const ego = (t: number) => range(t, 0.05, 0.45, easeOut) * (1 - range(t, 2.6, 4.6));

function holoState(t: number, frame: Frame): HoloState {
  const fold = range(t, FOLD[0], FOLD[1], easeInOut);
  const yaw = (0.55 * range(t, BAM, BAM + 1.6, easeOut) + 0.38 * range(t, BAM + 1.6, PUSH[0], linear)) * (1 - fold);
  const pitch = (0.36 * range(t, BAM, BAM + 1.4, easeOut) - 0.14 * range(t, BAM + 1.4, PUSH[0], easeInOut)) * (1 - fold);
  const sweep = (from: number, to: number) => (t >= from && t <= to ? lerp(frame.top - 60, frame.bottom + 60, (t - from) / (to - from)) : null);
  return {
    view: camera(t, frame),
    burst: t - BAM,
    flatten: fold,
    // the glow eases off as it folds, so lab-dreaming's flat first frame is what the fold lands on
    holo: range(t, BAM, BAM + 0.45, easeOut) * (1 - fold),
    yaw,
    pitch,
    // the new memory's links, lit as it lands
    ego: range(t, ARRIVE[1] - 0.15, ARRIVE[1] + 0.25, easeOut) * (1 - range(t, 5.4, 6.6)),
    scan: sweep(4.2, 6.2),
    ring: null,
    opacity: range(t, BAM - 0.02, BAM + 0.12, linear) * (1 - range(t, PUSH[1] - 0.2, PUSH[1] - 0.03, linear)),
    flight: 1.5,
    overshoot: 0,
    stagger: 0,
    arrival: range(t, ARRIVE[0], ARRIVE[1], linear),
  };
}

/** The flat graph: the whole cut when there is no WebGL, else only the frames that meet the neighbouring cuts. */
function flatLayer(t: number, frame: Frame, opacity: number): GraphLayer {
  const cam = camera(t, frame);
  const strength = ego(t);
  return {
    view: { cx: cam.x, cy: cam.y, scale: cam.scale, fx: frame.width / 2, fy: frame.height / 2 },
    positions: frame.nodes,
    highlight: { edges: HUB_EDGES, strength, width: 1.1 },
    lift: { nodes: HUB_NEIGHBOURS, amount: 1.5 * strength },
    opacity,
  };
}

const flatOpacity = (t: number) => 1 - range(t, BAM + 0.04, BAM + 0.24, linear) + range(t, PUSH[1] - 0.26, PUSH[1] - 0.06, linear);

/** A faint pool of the hologram's light under the volume, inside the frame. */
function Glow({ t, frame, state }: { t: number; frame: Frame; state: HoloState }) {
  const { view } = state;
  const cx = frame.nodes.reduce((s, n) => s + n[0], 0) / frame.nodes.length;
  const cy = frame.nodes.reduce((s, n) => s + n[1], 0) / frame.nodes.length;
  const x = frame.width / 2 + (cx - view.x) * view.scale;
  const y = frame.height / 2 + (cy - view.y) * view.scale;
  const r = 900 * view.scale;
  const k = state.holo * (1 - state.flatten) * state.opacity * (0.85 + 0.15 * Math.sin(t * 2.1));
  return <div aria-hidden="true" style={{ position: "absolute", inset: 0, opacity: k, background: `radial-gradient(${r}px ${r * 0.8}px at ${x}px ${y}px, rgba(46, 118, 255, 0.16), rgba(46, 118, 255, 0.05) 45%, transparent 75%)` }} />;
}

const fmt = (n: number) => n.toLocaleString("en-US");

type Type = { title: number; stats: number };
const LANDSCAPE_TYPE: Type = { title: 50, stats: 30 };
const PORTRAIT_TYPE: Type = { title: 56, stats: 30 };

/** Only what reads in a glance while the volume turns: whose memory and how big. */
function Title({ type }: { type: Type }) {
  return (
    <>
      <div style={{ fontSize: type.title, fontWeight: 600, color: "var(--ct-fg)", letterSpacing: "-0.015em", lineHeight: 1.1 }}>mac-mini · memory</div>
      <div style={{ marginTop: 14, fontSize: type.stats, color: "var(--ct-muted-fg)" }}>
        {fmt(totals.nodes)} nodes · {fmt(totals.edges)} relations
      </div>
    </>
  );
}

function MemoryGraph({ t }: { t: number }) {
  const portrait = useStage().portrait;
  const [holoReady, setHoloReady] = useState(false);
  const flatOnly = !holoReady;
  const light = themeIsLight();
  const f = portrait ? PORTRAIT : LANDSCAPE;
  const state = holoState(t, f);
  const leave = 1 - range(t, PUSH[0], PUSH[0] + 0.35);
  const title = range(t, 3.6, 4.4, easeOut) * leave;
  const scrim = title;
  const rise = (k: number): CSSProperties => ({ opacity: k, transform: `translateY(${(1 - k) * 12}px)` });
  const graph = (
    <>
      {!flatOnly && !light && <Glow t={t} frame={f} state={state} />}
      {/* Mounted only while it shows: every canvas here holds a WebGL context, and phones have few to spare. */}
      {(flatOnly || flatOpacity(t) > 0) && <StageCanvas t={t} layers={[flatLayer(t, f, flatOnly ? 1 : flatOpacity(t))]} />}
      <HoloCanvas t={t} state={state} layout={f.nodes} cast={CAST} onReady={setHoloReady} />
    </>
  );
  if (portrait) {
    return (
      <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)" }}>
        {graph}
        <div style={{ position: "absolute", left: 0, right: 0, top: 0, height: 420, opacity: scrim, background: "linear-gradient(180deg, var(--ct-bg) 0%, var(--ct-bg) 45%, transparent 100%)" }} />
        <div style={{ position: "absolute", left: 56, top: 140, width: 860, ...rise(title) }}>
          <Title type={PORTRAIT_TYPE} />
        </div>
      </div>
    );
  }
  return (
    <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)" }}>
      {graph}
      <div style={{ position: "absolute", top: 0, left: 0, width: 1100, height: 320, opacity: scrim, background: "radial-gradient(110% 100% at 0% 0%, var(--ct-bg) 0%, var(--ct-bg) 40%, transparent 100%)" }} />
      <div style={{ position: "absolute", left: 72, top: 64, ...rise(title) }}>
        <Title type={LANDSCAPE_TYPE} />
      </div>
    </div>
  );
}

const memoryGraph: Scene = {
  id: "memory-graph",
  duration: DURATION,
  render: (t) => <MemoryGraph t={t} />,
};

export default memoryGraph;
