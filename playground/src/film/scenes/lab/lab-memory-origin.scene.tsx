import type { CSSProperties } from "react";
import { MemoryNodeDetail } from "../../../../../src/components/MemoryPanel/MemoryNodeDetail";
import type { MemEdge, MemNode } from "../../../../../src/components/MemoryPanel/types";
import { API } from "../../../gallery/fixtures";
import { clamp, easeInOut, easeOut, lerp, range, useStage, type Scene } from "../../engine";
import { ago } from "../../fixtures/clock";
import { DOT_MATRIX_WAVE_GROWTH } from "../../../../../src/components/brand/dotMatrixModel";
import { BEAT_PEAK, beatLevel } from "../../../../../src/components/brand/useBrandMotion";
import { HoloCanvas } from "../mesh/holoMemory";
import { holoPalette, memoryColor, themeIsLight, type HoloState } from "../mesh/holoTheme";
import { placeOf, scaleAtDepth } from "../mesh/holoVolume";
import { holoFrame, HOOK_CAST, hookEnd, ORIGIN as HOOK_ORIGIN } from "./lab-memory-hook.scene";
import { EDGES, fmt, HUB, NODES, nodeRadius, project, rankFrom, StageCanvas, type GraphView } from "./labGraph";

const DURATION = 11;
/** A fact one link from memory-graph's hub, so the pull-back can settle on memory-graph's first frame. */
const ORIGIN = 221;
const RANK = rankFrom(ORIGIN);
const LOG_N = Math.log(NODES.length);
const REVEAL_AT = Array.from(RANK, (r) => (r === 0 ? 5.2 : 5.45 + 4.55 * (Math.log(1 + r) / LOG_N) ** 1.6));
const REVEAL_DUR = 0.35;

const OPEN_AT = 2.2;
const SHRINK_AT = 5.0;
const PULL = [5.4, 10.4] as const;
const START_SCALE = 22;
const CARD_W = 360;
const CARD_H = 330;

const iso = (minutes: number) => new Date(ago(minutes)).toISOString();

const mem = (id: string, kind: string, title: string, extra: Partial<MemNode> = {}): MemNode => ({
  id, kind, title, summary: null, area: null, importance: 6, tags: null, has_vector: true, has_body: true, created_at: iso(60 * 24 * 40), updated_at: iso(90), access_count: 7, ...extra,
});

const ORIGIN_MEM = mem("mem-owner-name", "fact", "The user's name is Sam", {
  summary: "Who the owner is and how they like to be answered.",
  body: "Prefers short, direct answers.",
  area: "identity",
  importance: 9,
  tags: ["owner", "identity"],
  created_at: iso(60 * 24 * 180),
  updated_at: iso(60 * 3),
  access_count: 214,
});

const NEIGHBOURS: Array<{ node: MemNode; rel: string; at: number }> = [
  { node: mem("mem-owner-peer", "peer", "Sam · owner of every machine"), rel: "about", at: 3.3 },
  { node: mem("mem-reply-language", "preference", "Reply in the language the user writes in"), rel: "relates_to", at: 3.8 },
  { node: mem("mem-no-preamble", "rule", "Lead with the answer, skip the preamble"), rel: "supports", at: 4.3 },
];

const CARD_STATES = NEIGHBOURS.map((_, count) => count).concat(NEIGHBOURS.length).map((count) => {
  const shown = NEIGHBOURS.slice(0, count);
  const edges: MemEdge[] = shown.map(({ node, rel }) => ({ from: ORIGIN_MEM.id, to: node.id, rel, confidence: 0.9, valid_from: null, valid_to: null }));
  return { edges, nodes: new Map(shown.map(({ node }) => [node.id, node])) };
});

const noop = () => undefined;

// The film keeps the memory itself (title, kind, summary, body, tags, links) and drops importance, stats, timestamps, id and actions.
const CARD_CSS = `.lab-memory-card [data-testid="memory-node-detail"] > :nth-child(5), .lab-memory-card [data-testid="memory-node-detail"] > :nth-child(n+8), .lab-memory-card [data-testid="memory-node-detail-tuck"] { display: none; }
.lab-memory-card [data-testid="memory-node-detail"] > :first-child strong { font-size: 17px; line-height: 1.25; }`;

/**
 * The same card dressed as a piece of the hologram it came from: the memory's orange for its kind, the hologram's blue
 * for its frame, chips and links, instead of the panel's grey and per-kind green.
 */
function holoCardCss(light: boolean, edges: number): string {
  const p = holoPalette(light);
  const rgb = (c: readonly number[], a = 1) => `rgba(${c.map((v) => Math.round(v * 255)).join(",")},${a})`;
  const d = `.lab-memory-card [data-testid="memory-node-detail"]`;
  return `${d} { background: transparent; border: 0; }
${d} > :first-child > :first-child { background: ${rgb(p.ego)} !important; box-shadow: 0 0 10px ${rgb(p.ego, light ? 0.35 : 0.7)}; }
${d} > :nth-child(2) > span:first-child { background: ${rgb(p.ego)} !important; color: ${light ? "#fff" : "#1a0d02"}; }
${d} > :nth-child(2) > span + span, ${d} > :nth-child(6) span { background: ${rgb(p.entity, light ? 0.12 : 0.14)} !important; color: ${light ? rgb(p.edge) : rgb(p.entity)}; }
${d} [data-testid="memory-node-body-preview"] { background: ${light ? "rgba(255,255,255,0.7)" : "rgba(4,10,22,0.55)"}; border: 1px solid ${rgb(p.edge, light ? 0.18 : 0.25)}; }
${d} [data-testid="memory-node-related-item"] > span:first-child { background: ${rgb(p.ego)} !important; }
${edges ? "" : `${d} > :nth-child(7) { display: none; }`}`;
}

/** The card's own frame: glassy, edged in the hologram's blue, lit from above in its orange. */
function holoCardFrame(light: boolean): CSSProperties {
  const p = holoPalette(light);
  const rgb = (c: readonly number[], a: number) => `rgba(${c.map((v) => Math.round(v * 255)).join(",")},${a})`;
  return light
    ? { border: `1px solid ${rgb(p.edge, 0.3)}`, background: `linear-gradient(180deg, ${rgb(p.ego, 0.06)}, rgba(255,255,255,0.96) 28%)` }
    : { border: `1px solid ${rgb(p.edge, 0.45)}`, background: `linear-gradient(180deg, ${rgb(p.ego, 0.1)}, rgba(14,22,38,0.94) 26%, rgba(9,14,26,0.96))`, boxShadow: `inset 0 1px 0 ${rgb(p.ego, 0.35)}` };
}

/** How far the card's blue glow reaches past its edge, in card pixels. */
const GLOW_REACH = 32;

/**
 * The card's blue glow, behind the card, never sliced by the circular reveal. While the circle opens the light is a soft
 * round halo that grows with it, with no edge of its own; once the card is fully open the halo hands over to the glow
 * along the card's outline.
 */
function CardGlow({ reveal, clipR, settled }: { reveal: number; clipR: number; settled: number }) {
  if (themeIsLight() || reveal <= 0) return null;
  const [r, g, b] = holoPalette(false).edge.map((v) => Math.round(v * 255));
  const halo = clipR + GLOW_REACH;
  return (
    <>
      {settled < 1 && (
        <div
          style={{
            position: "absolute", left: CARD_W / 2 - halo, top: CARD_H / 2 - halo, width: halo * 2, height: halo * 2, borderRadius: "50%",
            opacity: reveal * (1 - settled),
            background: `radial-gradient(closest-side, rgba(${r},${g},${b},0.17) ${Math.round((clipR / halo) * 100)}%, rgba(${r},${g},${b},0) 100%)`,
          }}
        />
      )}
      <div style={{ position: "absolute", left: 0, top: 0, width: CARD_W, height: CARD_H, borderRadius: 12, opacity: settled, boxShadow: `0 0 36px rgba(${r},${g},${b},0.24)` }} />
    </>
  );
}

// The shadow fades in only once the circular reveal is done; clipped by the circle it reads as a round halo.
function MemoryCard({ t, scale, shadow }: { t: number; scale: number; shadow: number }) {
  const state = CARD_STATES[NEIGHBOURS.filter((n) => t >= n.at).length];
  const light = themeIsLight();
  return (
    <div data-film-anchor="memory" style={{ position: "relative", width: CARD_W, height: CARD_H, transform: `scale(${scale})` }}>
      <div style={{ position: "absolute", inset: 0, borderRadius: 12, boxShadow: "var(--ct-shadow-xl)", opacity: shadow }} />
      <div className="lab-memory-card" style={{ position: "relative", width: CARD_W, height: CARD_H, borderRadius: 12, overflow: "hidden", ...holoCardFrame(light) }}>
        <style>{CARD_CSS + holoCardCss(light, state.edges.length)}</style>
        <MemoryNodeDetail api={API} node={ORIGIN_MEM} edges={state.edges} nodes={state.nodes} onUpdate={noop} onDelete={noop} onSelectNode={noop} />
      </div>
    </div>
  );
}

function layout(portrait: boolean) {
  return portrait
    ? { fx: 540, fy: 768, cardScale: 2.4, endScale: 1.9, dot: 16, titleLeft: 64, titleTop: 110 }
    : { fx: 960, fy: 540, cardScale: 2.75, endScale: 3.1, dot: 14, titleLeft: 72, titleTop: 64 };
}

function cameraAt(t: number, fx: number, fy: number, endScale: number): GraphView {
  const k = range(t, PULL[0], PULL[1]);
  const [ox, oy] = NODES[ORIGIN];
  const [hx, hy] = NODES[HUB];
  return { cx: lerp(ox, hx, k), cy: lerp(oy, hy, k), scale: Math.exp(lerp(Math.log(START_SCALE), Math.log(endScale), k)), fx, fy };
}

function revealAlpha(t: number) {
  const alpha = new Float32Array(NODES.length);
  for (let i = 0; i < NODES.length; i++) alpha[i] = clamp((t - REVEAL_AT[i]) / REVEAL_DUR);
  return alpha;
}

/**
 * "It starts with one memory": the opening's volume, picked up exactly where lab-memory-hook left it, gathers back into
 * that one memory. The far clusters let go first and everything accelerates into the point, brightening as it lands,
 * while the turn eases out and the camera keeps the memory in view; the beacon is what remains.
 */
const GATHER = [0, 1.9] as const;
const GATHER_STAGGER = 3;
const GATHER_FLIGHT = 0.9;
/** The burst parameter at which every node is still in place under the gather's stagger and flight (delays reach 0.34). */
const GATHER_TOP = 0.34 * GATHER_STAGGER + GATHER_FLIGHT;

function gatherState(t: number, portrait: boolean): HoloState {
  const end = hookEnd(portrait);
  const f = holoFrame(portrait);
  const unwind = range(t, 0, 1.6, easeInOut);
  const yaw = end.yaw * (1 - unwind);
  const pitch = end.pitch * (1 - unwind);
  const at = placeOf(HOOK_ORIGIN, f.nodes, 0, yaw, pitch);
  const scale = end.view.scale * (1 + 0.25 * range(t, 0, 1.9, easeInOut));
  // The memory glides to where the beacon will stand (the frame centre, higher in portrait).
  const eff = scaleAtDepth(scale, f.height, at.z);
  const land = range(t, 0.2, 1.7, easeInOut);
  const blend = range(t, 0, 0.6, easeOut);
  const x = lerp(end.view.x, at.x + ((f.width / 2 - f.fx) / eff) * land, blend);
  const y = lerp(end.view.y, at.y + ((f.height / 2 - f.fy) / eff) * land, blend);
  return {
    ...end,
    view: { x, y, scale },
    yaw,
    pitch,
    ring: null,
    wave: null,
    burst: lerp(GATHER_TOP, -0.05, range(t, GATHER[0], GATHER[1], (k) => k)),
    stagger: GATHER_STAGGER,
    flight: GATHER_FLIGHT,
    overshoot: 0,
    fromNode: HOOK_ORIGIN,
    opacity: 1 - range(t, GATHER[1] - 0.12, GATHER[1] + 0.05, (k) => k),
  };
}

const BEAT_PERIOD = 0.95;
const FIRST_BEAT = OPEN_AT - 2 * BEAT_PERIOD;
const WAVES = [0, 1, 2].map((i) => FIRST_BEAT + i * BEAT_PERIOD + BEAT_PEAK * BEAT_PERIOD);
const WAVE_DOTS = 40;

/** One accent dot beating like the product's dot matrix, each beat releasing a ring of dots. */
function Beacon({ t, x, y, r }: { t: number; x: number; y: number; r: number }) {
  const s = t - FIRST_BEAT;
  const beat = s < 0 ? 0 : beatLevel((s % BEAT_PERIOD) / BEAT_PERIOD);
  // It comes up as the gathered volume lands in it.
  const arrive = range(t, GATHER[1] - 0.4, GATHER[1], easeOut);
  const open = range(t, OPEN_AT, OPEN_AT + 0.35, easeOut);
  return (
    <svg style={{ position: "absolute", inset: 0, width: "100%", height: "100%", overflow: "visible" }} aria-hidden="true">
      {WAVES.map((at) => {
        const k = (t - at) / 1.6;
        if (k < 0 || k > 1) return null;
        const radius = r * (1.6 + 16 * easeOut(k));
        return Array.from({ length: WAVE_DOTS }, (_, i) => {
          const a = (i / WAVE_DOTS) * Math.PI * 2;
          return <circle key={`${at}-${i}`} cx={x + Math.cos(a) * radius} cy={y + Math.sin(a) * radius} r={r * 0.22 * (1 + DOT_MATRIX_WAVE_GROWTH * (1 - k))} fill={memoryColor()} opacity={0.75 * (1 - k) * arrive} />;
        });
      })}
      <circle cx={x} cy={y} r={r * (2.2 + 1.2 * beat)} fill={memoryColor()} opacity={(0.1 + 0.12 * beat) * arrive * (1 - open)} />
      <circle cx={x} cy={y} r={r * (0.9 + 0.35 * beat) * lerp(0.4, 1, arrive) * (1 + 3 * open)} fill={memoryColor()} opacity={arrive * (1 - open)} />
    </svg>
  );
}

function MemoryOrigin({ t }: { t: number }) {
  const { portrait } = useStage();
  const L = layout(portrait);
  const view = cameraAt(t, L.fx, L.fy, L.endScale);
  const alpha = revealAlpha(t);
  const [ox, oy] = project(view, NODES[ORIGIN][0], NODES[ORIGIN][1]);
  const originR = nodeRadius(NODES[ORIGIN], view.scale);

  const opening = range(t, OPEN_AT, OPEN_AT + 0.8);
  const shrink = range(t, SHRINK_AT, SHRINK_AT + 0.6, easeInOut);
  const cardScale = Math.exp(lerp(Math.log(L.cardScale), Math.log((originR * 2.4) / CARD_W), shrink));
  const cardOpacity = 1 - range(t, SHRINK_AT + 0.3, SHRINK_AT + 0.6);
  const clipR = lerp(L.dot / L.cardScale, Math.hypot(CARD_W, CARD_H) / 2 + 4, opening);

  const ring = range(t, 5.4, 5.8, easeOut) * (1 - range(t, 9.8, 10.4));
  const counter = range(t, 5.6, 6.0, easeOut) * (1 - range(t, 9.9, 10.4));
  let shownNodes = 0;
  for (const a of alpha) if (a >= 0.5) shownNodes++;
  let shownEdges = 0;
  for (const [a, b] of EDGES) if (alpha[a] >= 0.5 && alpha[b] >= 0.5) shownEdges++;

  return (
    <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)" }}>
      {t < GATHER[1] + 0.1 && <HoloCanvas t={t} state={gatherState(t, portrait)} layout={holoFrame(portrait).nodes} cast={HOOK_CAST} onReady={noop} />}
      {t >= 5.1 && <StageCanvas t={t} layers={[{ view, nodeAlpha: alpha }]} />}
      {ring > 0 && (
        <svg style={{ position: "absolute", inset: 0, width: "100%", height: "100%", overflow: "visible" }} aria-hidden="true">
          <circle cx={ox} cy={oy} r={originR + 10 + 6 * (1 - ring)} fill="none" stroke={memoryColor()} strokeWidth={2.5} opacity={0.9 * ring} />
          <circle cx={ox} cy={oy} r={originR + 26} fill="none" stroke={memoryColor()} strokeWidth={1} opacity={0.35 * ring} />
        </svg>
      )}
      {t < OPEN_AT + 1.7 && <Beacon t={t} x={L.fx} y={L.fy} r={L.dot} />}
      {t >= OPEN_AT && cardOpacity > 0 && (
        <div style={{ position: "absolute", left: L.fx - CARD_W / 2, top: L.fy - CARD_H / 2, width: CARD_W, height: CARD_H, opacity: cardOpacity }}>
          <div style={{ position: "relative", width: CARD_W, height: CARD_H, transform: `scale(${cardScale / L.cardScale})` }}>
            <div style={{ position: "absolute", inset: 0, transform: `scale(${L.cardScale})` }}>
              <CardGlow reveal={range(t, OPEN_AT, OPEN_AT + 0.3, easeOut)} clipR={clipR} settled={range(t, OPEN_AT + 0.5, OPEN_AT + 0.9, easeInOut)} />
            </div>
            <div style={{ position: "absolute", inset: 0, clipPath: opening < 1 ? `circle(${clipR * L.cardScale}px at 50% 50%)` : "none" }}>
              <MemoryCard t={t} scale={L.cardScale} shadow={range(t, OPEN_AT + 0.8, OPEN_AT + 1.2)} />
            </div>
          </div>
        </div>
      )}
      {counter > 0 && (
        <div style={{ position: "absolute", left: L.titleLeft, top: L.titleTop, opacity: counter, transform: `translateY(${(1 - counter) * 12}px)` }}>
          <div style={{ fontSize: portrait ? 40 : 30, fontWeight: 600, color: "var(--ct-fg)", letterSpacing: "-0.01em" }}>mac-mini · memory</div>
          <div style={{ marginTop: 10, fontSize: portrait ? 30 : 21, color: "var(--ct-muted-fg)", fontVariantNumeric: "tabular-nums" }}>
            {fmt(shownNodes)} nodes · {fmt(shownEdges)} relations
          </div>
        </div>
      )}
    </div>
  );
}

const memoryOrigin: Scene = {
  id: "lab-memory-origin",
  duration: DURATION,
  render: (t) => <MemoryOrigin t={t} />,
};

export default memoryOrigin;
