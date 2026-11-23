import { Moon, Search } from "lucide-react";
import type { CSSProperties } from "react";
import { ConnectionLine } from "../../../../../src/components/MeshPanel/ConnectionLine";
import { DreamCard, type DreamDiaryEntry } from "../../../../../src/components/MemoryPanel/DreamCard";
import { summarizeDreamLog } from "../../../../../src/components/MemoryPanel/dreamVisuals";
import { clamp, easeInOut, easeOut, lerp, range, Spotlight, useStage, type FocusKey, type Scene } from "../../engine";
import { FILM_NOW } from "../../fixtures/clock";
import { themeIsLight } from "../mesh/holoTheme";
import { Pulse, Ripple, Typed, type PeerName } from "../mesh/meshFixtures";
import { ADJACENCY, EDGES, hash, NODES, project, StageCanvas, type GraphView } from "./labGraph";

const DURATION = 11;
const PULL = [5.6, 7.0] as const;
const CENTRE: [number, number] = [-545, -205];
const CENTROID: [number, number] = [13, 7];
const inRegion = ([x, y]: readonly number[], pad = 0) => Math.abs(x - CENTRE[0]) < 85 + pad && Math.abs(y - CENTRE[1]) < 55 + pad;
const dist = (a: number, b: number) => Math.hypot(NODES[a][0] - NODES[b][0], NODES[a][1] - NODES[b][1]);
const linked = (a: number, b: number) => ADJACENCY[a].includes(b);
const byHash = (a: number, b: number) => hash(a, 3) - hash(b, 3);

const REGION = NODES.map((_, i) => i).filter((i) => inRegion(NODES[i]));
const KINDED = REGION.filter((i) => NODES[i][2] >= 0).sort(byHash);

type Merge = { into: number; from: number; at: number };
const MERGES: Merge[] = [];
{
  const used = new Set<number>();
  for (const a of KINDED) {
    if (MERGES.length === 4) break;
    if (used.has(a) || !inRegion(NODES[a], -15)) continue;
    const b = KINDED.find((c) => c !== a && !used.has(c) && NODES[c][2] === NODES[a][2] && dist(a, c) < 9 && dist(a, c) > 3);
    if (b === undefined) continue;
    used.add(a).add(b);
    MERGES.push({ into: a, from: b, at: 0.6 + MERGES.length * 0.85 });
  }
}
const MERGED = new Set(MERGES.flatMap((m) => [m.into, m.from]));
const MERGE_DUR = 0.8;

const STALE = EDGES.map((e, i) => ({ e, i }))
  .filter(({ e: [a, b] }) => inRegion(NODES[a], -10) && inRegion(NODES[b], -10) && !MERGED.has(a) && !MERGED.has(b) && dist(a, b) > 12)
  .sort((x, y) => byHash(x.i, y.i))
  .slice(0, 6)
  .map(({ i }, k) => ({ edge: i, at: 1.0 + k * 0.42 }));

const NEW_LINKS: Array<{ a: number; b: number; at: number }> = [];
for (const a of KINDED) {
  if (NEW_LINKS.length === 6) break;
  if (MERGED.has(a) || NEW_LINKS.some((l) => l.a === a || l.b === a) || !inRegion(NODES[a], -10)) continue;
  const b = KINDED.find((c) => c !== a && !MERGED.has(c) && !linked(a, c) && dist(a, c) > 18 && dist(a, c) < 55 && !NEW_LINKS.some((l) => l.a === c || l.b === c));
  if (b !== undefined) NEW_LINKS.push({ a, b, at: 1.7 + NEW_LINKS.length * 0.45 });
}

const GLOWS = EDGES.map((_, i) => i)
  .filter((i) => inRegion(NODES[EDGES[i][0]], -5) && inRegion(NODES[EDGES[i][1]], -5))
  .sort((x, y) => hash(x, 9) - hash(y, 9))
  .slice(0, 12)
  .map((edge, k) => ({ edge, at: 0.2 + k * 0.38 }));

/**
 * The dream diary card in the film's blue rather than its per-dream colour (purple for this one), so it belongs with the
 * hologram and the site. Subtle tint, and every line (date, mood pill, italic body, tags) kept well above 4.5:1.
 */
function dreamCardCss(light: boolean): string {
  const c = `.film-dream-card [data-testid="memory-dream-card"]`;
  const t = light
    ? { surface: "linear-gradient(160deg, #eef5ff 0%, #e2edfc 100%)", shadow: "0 8px 24px -12px rgba(9,92,196,0.35), inset 0 0 0 1px rgba(9,92,196,0.24)", blob: "rgba(9,92,196,0.22)", accent: "#095cc4", tint: "rgba(9,92,196,0.11)", edge: "rgba(9,92,196,0.3)", ink: "#0b2545", pill: "#07489a", rule: "rgba(9,92,196,0.14)" }
    : { surface: "linear-gradient(160deg, #10284a 0%, #0b1a2f 100%)", shadow: "0 8px 24px -12px rgba(88,166,255,0.4), inset 0 0 0 1px rgba(88,166,255,0.26)", blob: "rgba(88,166,255,0.5)", accent: "#79b8ff", tint: "rgba(88,166,255,0.14)", edge: "rgba(88,166,255,0.36)", ink: "#e6edf3", pill: "#b3d7ff", rule: "rgba(88,166,255,0.14)" };
  return `${c} { background: ${t.surface} !important; box-shadow: ${t.shadow} !important; }
${c} > div:first-child { background: radial-gradient(circle at center, ${t.blob} 0%, transparent 70%) !important; }
${c} > div:nth-child(2) > div:first-child { background: ${t.tint} !important; border-color: ${t.edge} !important; color: ${t.accent} !important; }
${c} > div:nth-child(2) > div:last-child > div { color: ${t.ink} !important; }
${c} > div:nth-child(3) > span:first-child { background: ${t.tint} !important; color: ${t.pill} !important; }
${c} [data-testid="memory-dream-card-body"] { color: ${t.ink} !important; opacity: 1 !important; }
${c} [data-testid="memory-dream-card-tag"] { background: ${t.tint} !important; color: ${t.ink} !important; opacity: 0.9 !important; }
${c} > div:last-child { border-top-color: ${t.rule} !important; }`;
}

function dreamLog(t: number) {
  const merged = MERGES.filter((m) => t >= m.at + MERGE_DUR).length;
  const stale = STALE.filter((s) => t >= s.at + 0.7).length;
  const linkedNow = NEW_LINKS.filter((l) => t >= l.at + 0.6).length;
  return summarizeDreamLog(`PARALLEL_EDGES: ${stale}\nORPHANS: bridged=${linkedNow}\nDEDUP: duplicates=${merged}`);
}

const DREAM: DreamDiaryEntry = {
  id: "dream-mac-mini-night",
  title: "Night pass on mac-mini",
  body: "<insight>Four notes about the gateway cert were one memory said four ways; they are one now. The old relay route no longer holds, so its links are gone, and the renewal now points at the gateway it belongs to.</insight><mood>tidy</mood><icon>moon</icon>",
  created_at: new Date(FILM_NOW).toISOString(),
  area: "infra",
  tags: ["dream-diary", "dedup", "relink"],
};

type Machine = { peer: PeerName; x: number; y: number; r: number; turn: number; keep: number; at: number };

function machines(portrait: boolean): Machine[] {
  const m = (peer: PeerName, x: number, y: number, r: number, i: number): Machine => ({ peer, x, y, r, turn: (hash(i, 5) % 628) / 100, keep: 35 + (hash(i, 6) % 30), at: 6.5 + i * 0.12 });
  return portrait
    ? [m("mac-mini", 540, 820, 175, 0), m("laptop", 250, 330, 120, 1), m("gpu-rig", 830, 330, 120, 2), m("homelab", 245, 1250, 115, 3), m("studio", 835, 1250, 115, 4), m("pi-hub", 540, 1330, 80, 5)]
    : [m("mac-mini", 960, 560, 195, 0), m("laptop", 360, 300, 130, 1), m("gpu-rig", 1560, 300, 130, 2), m("homelab", 360, 830, 125, 3), m("studio", 1560, 830, 125, 4), m("pi-hub", 960, 130, 70, 5)];
}

const MESH_LINKS: Array<[PeerName, PeerName]> = [["laptop", "mac-mini"], ["gpu-rig", "mac-mini"], ["homelab", "mac-mini"], ["studio", "mac-mini"], ["pi-hub", "mac-mini"], ["laptop", "gpu-rig"], ["homelab", "studio"], ["laptop", "homelab"], ["gpu-rig", "studio"], ["pi-hub", "laptop"], ["pi-hub", "gpu-rig"]];

const ASKS: Array<{ from: PeerName; to: PeerName; at: number }> = [
  { from: "laptop", to: "mac-mini", at: 7.3 },
  { from: "laptop", to: "gpu-rig", at: 7.5 },
  { from: "studio", to: "homelab", at: 8.4 },
  { from: "pi-hub", to: "mac-mini", at: 8.7 },
  { from: "gpu-rig", to: "studio", at: 9.3 },
  { from: "homelab", to: "mac-mini", at: 9.6 },
];
const TRAVEL = 0.6;
const ANSWER_GAP = 0.1;

/** Points on the two clouds' rims facing each other. */
function rims(a: Machine, b: Machine): [[number, number], [number, number]] {
  const d = Math.hypot(b.x - a.x, b.y - a.y);
  const ux = (b.x - a.x) / d;
  const uy = (b.y - a.y) / d;
  return [[a.x + ux * (a.r + 14), a.y + uy * (a.r + 14)], [b.x - ux * (b.r + 14), b.y - uy * (b.r + 14)]];
}

function layout(portrait: boolean) {
  return portrait
    ? { fx: 540, fy: 660, s0: 6.6, s1: 7.1, label: { left: 64, top: 110 }, title: 40, sub: 30, card: { left: 110, top: 1100, scale: 1.3 } }
    : { fx: 760, fy: 580, s0: 7.0, s1: 7.6, label: { left: 72, top: 64 }, title: 30, sub: 24, card: { left: 1450, top: 330, scale: 1.45 } };
}

function mainView(t: number, portrait: boolean, home: Machine): GraphView {
  const L = layout(portrait);
  const drift = range(t, 0, PULL[0], (k) => k);
  const pull = range(t, PULL[0], PULL[1], easeInOut);
  const nightScale = lerp(L.s0, L.s1, drift);
  const cloudScale = home.r / 1180;
  return {
    cx: lerp(CENTRE[0], CENTROID[0], pull),
    cy: lerp(CENTRE[1], CENTROID[1], pull),
    scale: Math.exp(lerp(Math.log(nightScale), Math.log(cloudScale), pull)),
    fx: lerp(L.fx, home.x, pull),
    fy: lerp(L.fy, home.y, pull),
  };
}

// Per-frame arrays are reused, never reallocated: on a phone tens of kilobytes of fresh arrays every frame keep the
// garbage collector busy enough to stutter. A frame's layers are drawn within the same commit, so one buffer each is safe.
const CLOUD_ALPHA = new Map<number, Float32Array>();

function cloudAlpha(m: Machine, t: number) {
  let alpha = CLOUD_ALPHA.get(m.turn);
  if (!alpha) CLOUD_ALPHA.set(m.turn, (alpha = new Float32Array(NODES.length)));
  for (let i = 0; i < NODES.length; i++) alpha[i] = hash(i, m.turn * 100) % 100 < m.keep ? clamp((t - m.at - (hash(i, 11) % 100) / 160) / 0.3) : 0;
  return alpha;
}

/** Theme colours are read once per theme, not with a style recalc every frame. */
let cssCache: { key: string; values: Map<string, string> } | null = null;
function css(name: string) {
  const root = document.documentElement;
  const key = `${root.getAttribute("data-ct-theme")}|${root.className}|${root.getAttribute("style")}`;
  if (cssCache?.key !== key) cssCache = { key, values: new Map() };
  let value = cssCache.values.get(name);
  if (value === undefined) cssCache.values.set(name, (value = getComputedStyle(root).getPropertyValue(name).trim()));
  return value;
}

function drawNight(context: CanvasRenderingContext2D, view: GraphView, t: number, fade: number) {
  const accent = css("--ct-accent");
  const red = css("--ct-red");
  const at = (i: number, moved?: [number, number]) => project(view, moved ? moved[0] : NODES[i][0], moved ? moved[1] : NODES[i][1]);
  context.lineCap = "round";

  for (const { edge, at: start } of STALE) {
    const k = range(t, start, start + 0.7, (x) => x);
    if (k <= 0 || k >= 1) continue;
    const [a, b] = EDGES[edge];
    const [ax, ay] = at(a);
    const [bx, by] = at(b);
    context.strokeStyle = red;
    context.globalAlpha = fade * Math.sin(Math.PI * k) * 0.9;
    context.lineWidth = 2.5;
    context.setLineDash([6, 6]);
    context.beginPath();
    context.moveTo(ax, ay);
    context.lineTo(bx, by);
    context.stroke();
  }
  context.setLineDash([]);

  for (const { a, b, at: start } of NEW_LINKS) {
    const grow = range(t, start, start + 0.6, easeOut);
    if (grow <= 0) continue;
    const [ax, ay] = at(a);
    const [bx, by] = at(b);
    const settle = range(t, start + 0.6, start + 1.6);
    context.strokeStyle = accent;
    context.globalAlpha = fade * lerp(0.9, 0.35, settle);
    context.lineWidth = lerp(2.6, 1.4, settle);
    context.beginPath();
    context.moveTo(ax, ay);
    context.lineTo(lerp(ax, bx, grow), lerp(ay, by, grow));
    context.stroke();
  }

  for (const { edge, at: start } of GLOWS) {
    const k = range(t, start, start + 0.9, easeInOut);
    if (k <= 0 || k >= 1) continue;
    const [ax, ay] = at(EDGES[edge][0]);
    const [bx, by] = at(EDGES[edge][1]);
    const x = lerp(ax, bx, k);
    const y = lerp(ay, by, k);
    context.fillStyle = accent;
    context.globalAlpha = fade * 0.2;
    context.beginPath();
    context.arc(x, y, 14, 0, Math.PI * 2);
    context.fill();
    context.globalAlpha = fade * 0.95;
    context.beginPath();
    context.arc(x, y, 4.5, 0, Math.PI * 2);
    context.fill();
  }

  for (const m of MERGES) {
    const k = range(t, m.at + MERGE_DUR, m.at + MERGE_DUR + 0.9, easeOut);
    const pull = range(t, m.at, m.at + MERGE_DUR);
    const [x, y] = at(m.into);
    context.strokeStyle = accent;
    if (pull > 0 && k < 1) {
      context.globalAlpha = fade * 0.8 * (1 - k);
      context.lineWidth = 2;
      context.beginPath();
      context.arc(x, y, 12 + 30 * k, 0, Math.PI * 2);
      context.stroke();
    }
  }
  context.globalAlpha = 1;
}

function movedAt(t: number) {
  const moved = new Map<number, [number, number]>();
  for (const m of MERGES) {
    const k = range(t, m.at, m.at + MERGE_DUR, easeInOut);
    if (k > 0) moved.set(m.from, [lerp(NODES[m.from][0], NODES[m.into][0], k), lerp(NODES[m.from][1], NODES[m.into][1], k)]);
  }
  return moved;
}

const EDGE_ALPHA = new Float32Array(EDGES.length).fill(1);

/** Only the few stale edges ever change, so only they are written; null until the first one starts to fade. */
function edgeAlphaAt(t: number) {
  if (t < STALE[0]?.at + 0.2) return undefined;
  for (const { edge, at } of STALE) EDGE_ALPHA[edge] = 1 - range(t, at + 0.2, at + 0.7);
  return EDGE_ALPHA;
}

function rectAround(points: Array<[number, number]>, pad: number): FocusKey["rect"] {
  const xs = points.map((p) => p[0]);
  const ys = points.map((p) => p[1]);
  const x = Math.min(...xs) - pad;
  const y = Math.min(...ys) - pad;
  return { x, y, w: Math.max(...xs) + pad - x, h: Math.max(...ys) + pad - y };
}

function Dreaming({ t }: { t: number }) {
  const { portrait } = useStage();
  const L = layout(portrait);
  const ms = machines(portrait);
  const home = ms[0];
  const peer = (name: PeerName) => ms.find((m) => m.peer === name)!;
  const view = mainView(t, portrait, home);
  const night = 1 - range(t, PULL[0] - 0.2, PULL[0] + 0.4);
  const mesh = range(t, 6.6, 7.2, easeOut);

  const merge0 = project(mainView(1.0, portrait, home), NODES[MERGES[0].into][0], NODES[MERGES[0].into][1]);
  const link = NEW_LINKS[1] ?? NEW_LINKS[0];
  const linkView = mainView(2.6, portrait, home);
  const linkRect = rectAround([project(linkView, NODES[link.a][0], NODES[link.a][1]), project(linkView, NODES[link.b][0], NODES[link.b][1])], 46);
  const card = { x: L.card.left - 10, y: L.card.top - 10, w: 260 * L.card.scale + 20, h: 280 * L.card.scale + 20 };
  const laptop = peer("laptop");
  const mergeRect = { x: merge0[0] - 100, y: merge0[1] - 100, w: 200, h: 200 };
  const focus: FocusKey[] = [
    { t: 3.9 },
    { t: 4.3, rect: card, dim: 0.5 },
    { t: 5.3, rect: card, dim: 0.5 },
    { t: 5.9 },
  ];
  const labels: Array<{ from: number; to: number; rect: NonNullable<FocusKey["rect"]>; text: string }> = [
    { from: 0.7, to: 1.9, rect: mergeRect, text: "Duplicates fold into one" },
    { from: 2.5, to: 3.7, rect: linkRect!, text: "Links re-route" },
  ];

  const summary = dreamLog(t);
  const dreamCard = range(t, 4.0, 4.5, easeOut) * (1 - range(t, PULL[0], PULL[0] + 0.35));
  const query = range(t, 6.9, 7.2, easeOut) * (1 - range(t, 9.6, 10.0));

  return (
    <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)" }}>
      <svg style={{ position: "absolute", inset: 0, width: "100%", height: "100%", opacity: mesh }} aria-hidden="true">
        {ms.map((m) => <circle key={m.peer} cx={m.x} cy={m.y} r={m.r * 1.08} fill="var(--ct-accent)" opacity={0.05} />)}
      </svg>
      <StageCanvas
        t={t}
        layers={[
          { view, moved: movedAt(t), edgeAlpha: edgeAlphaAt(t), opacity: lerp(1, 0.85, night), edgeBoost: lerp(1, 0.75, night) + 1.6 * range(t, PULL[0], PULL[1]) },
          ...ms.slice(1).filter((m) => t >= m.at).map((m) => ({ view: { cx: CENTROID[0], cy: CENTROID[1], scale: m.r / 1180, fx: m.x, fy: m.y, rotate: m.turn }, nodeAlpha: cloudAlpha(m, t), edgeBoost: 2.6 })),
        ]}
        overlay={night > 0 ? (context) => drawNight(context, view, t, night) : undefined}
      />
      <svg style={{ position: "absolute", inset: 0, width: "100%", height: "100%", overflow: "visible" }} aria-hidden="true">
        <g opacity={mesh}>
          {MESH_LINKS.map(([a, b]) => {
            const [p, q] = rims(peer(a), peer(b));
            const busy = ASKS.some((ask) => ((ask.from === a && ask.to === b) || (ask.from === b && ask.to === a)) && t >= ask.at && t < ask.at + 2 * TRAVEL + ANSWER_GAP);
            return <ConnectionLine key={`${a}-${b}`} fromX={p[0]} fromY={p[1]} toX={q[0]} toY={q[1]} pushCount={busy ? 7 : 1} pulsing={false} />;
          })}
        </g>
        {ASKS.map((ask) => {
          const [p, q] = rims(peer(ask.from), peer(ask.to));
          const back = ask.at + TRAVEL + ANSWER_GAP;
          return (
            <g key={`${ask.from}-${ask.to}`}>
              <Pulse t={t} from={p} to={q} start={ask.at} end={ask.at + TRAVEL} r={5} />
              <Ripple t={t} x={q[0]} y={q[1]} at={ask.at + TRAVEL} size={28} />
              <Pulse t={t} from={q} to={p} start={back} end={back + TRAVEL} r={8} />
              <Ripple t={t} x={peer(ask.from).x} y={peer(ask.from).y} at={back + TRAVEL} size={peer(ask.from).r * 0.8} />
            </g>
          );
        })}
      </svg>
      {ms.map((m) => (
        <div key={m.peer} style={{ position: "absolute", left: m.x, top: m.y + m.r + 18, transform: "translateX(-50%)", opacity: range(t, m.at, m.at + 0.5, easeOut) * mesh, fontSize: portrait ? 30 : 26, fontWeight: 600, color: "var(--ct-fg)", whiteSpace: "nowrap" }}>
          {m.peer}
        </div>
      ))}
      {query > 0 && (
        <div style={chip(laptop, portrait, query)}>
          <Search size={portrait ? 26 : 22} color="var(--ct-muted-fg)" />
          <Typed t={t} at={7.0} cps={30} text="gateway cert renewal" />
        </div>
      )}
      {dreamCard > 0 && (
        <div data-film-hint="Dream diary: what memory merged and cleaned up overnight" style={{ position: "absolute", left: L.card.left, top: L.card.top, opacity: dreamCard, transform: `translateY(${(1 - dreamCard) * 16}px) scale(${L.card.scale})`, transformOrigin: "0 0" }}>
          <div className="film-dream-card">
            <style>{dreamCardCss(themeIsLight())}</style>
            <DreamCard entry={DREAM} onOpen={() => undefined} />
          </div>
        </div>
      )}
      <Spotlight t={t} keys={focus} />
      {labels.map(({ from, to, rect, text }) => {
        const k = range(t, from, from + 0.3, easeOut) * (1 - range(t, to, to + 0.3));
        if (k <= 0) return null;
        return <div key={text} style={{ position: "absolute", left: rect.x, top: rect.y + rect.h + 14, opacity: k, transform: `translateY(${(1 - k) * 8}px)`, zIndex: 60, fontSize: portrait ? 30 : 26, fontWeight: 600, color: "var(--ct-fg)", whiteSpace: "nowrap" }}>{text}</div>;
      })}
      {night > 0 && (
        <div style={{ position: "absolute", left: L.label.left, top: L.label.top, opacity: night * range(t, 0.1, 0.6, easeOut), zIndex: 60 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 14, fontSize: L.title, fontWeight: 600, color: "var(--ct-fg)", letterSpacing: "-0.01em" }}>
            <Moon size={L.title} color="var(--ct-accent)" />
            mac-mini · dreaming
            <span style={{ fontWeight: 400, color: "var(--ct-muted-fg)", fontFamily: "var(--ct-font-mono)", fontSize: L.sub }}>03:12</span>
          </div>
          <div style={{ marginTop: 12, maxWidth: portrait ? 860 : 1100, fontSize: L.sub, color: "var(--ct-muted-fg)", minHeight: L.sub * 1.4 }}>{summary ?? "Reading the day's memories…"}</div>
        </div>
      )}
    </div>
  );
}

function chip(laptop: Machine, portrait: boolean, k: number): CSSProperties {
  return {
    position: "absolute",
    left: portrait ? laptop.x + 70 : laptop.x,
    top: laptop.y + laptop.r + (portrait ? 66 : 58),
    transform: `translateX(-50%) translateY(${(1 - k) * 10}px)`,
    opacity: k,
    display: "flex",
    alignItems: "center",
    gap: 10,
    padding: "10px 16px",
    borderRadius: 10,
    border: "1px solid var(--ct-accent)",
    background: "var(--ct-bg)",
    fontSize: portrait ? 28 : 24,
    color: "var(--ct-fg)",
    whiteSpace: "nowrap",
    zIndex: 3,
  };
}

const dreaming: Scene = {
  id: "lab-dreaming",
  duration: DURATION,
  render: (t) => <Dreaming t={t} />,
};

export default dreaming;
