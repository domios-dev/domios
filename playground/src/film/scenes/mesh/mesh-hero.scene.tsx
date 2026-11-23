import { Lock } from "lucide-react";
import { ConnectionLine } from "../../../../../src/components/MeshPanel/ConnectionLine";
import { Camera, easeOut, lerp, range, useStage, type Key, type Scene } from "../../engine";
import { GUEST_JOINS, useGuest } from "../../guest";
import { Appear, At, H, MachineCard, meshBackground, PH, Pulse, PW, SAFE_X, W, type PeerName } from "./meshFixtures";

export const RING: PeerName[] = ["laptop", "gpu-rig", "pi-hub", "mac-mini", "studio", "homelab"];
const appearAt = (i: number) => -0.3 + i * 0.32;

export type Node = { peer: PeerName; x: number; y: number; at: number };
export type Layout = { width: number; height: number; nodes: Node[]; camera: Key[]; badge: { x: number; y: number; font: number }; card: number; nameSize: number; pulse: number };

const RX = 660;
const RY = 345;

const LANDSCAPE_NODES = RING.map((peer, i) => {
  const a = Math.PI + (i * Math.PI) / 3;
  return { peer, x: W / 2 + RX * Math.cos(a), y: H / 2 + RY * Math.sin(a) * 1.08, at: appearAt(i) };
});

export const LANDSCAPE: Layout = {
  width: W,
  height: H,
  nodes: LANDSCAPE_NODES,
  camera: [{ t: 0, x: LANDSCAPE_NODES[0].x + 60, y: H / 2, scale: 1.4 }, { t: 6.6, x: W / 2, y: H / 2, scale: 1.0 }, { t: 8, x: W / 2, y: H / 2, scale: 0.985 }],
  badge: { x: W / 2, y: H / 2, font: 32 },
  card: 1.42,
  nameSize: 19,
  pulse: 8,
};

const COL = [264, 720];
const ROW = [470, 860, 1250];
const GRID: Record<PeerName, [col: number, row: number]> = { "gpu-rig": [0, 0], "pi-hub": [1, 0], laptop: [0, 1], "mac-mini": [1, 1], homelab: [0, 2], studio: [1, 2] };
const P_CENTRE_X = (COL[0] + COL[1]) / 2;
const PORTRAIT_NODES =RING.map((peer, i) => ({ peer, x: COL[GRID[peer][0]], y: ROW[GRID[peer][1]], at: appearAt(i) }));

export const PORTRAIT: Layout = {
  width: PW,
  height: PH,
  nodes: PORTRAIT_NODES,
  camera: [{ t: 0, x: P_CENTRE_X, y: ROW[1], scale: 1.06 }, { t: 6.6, x: P_CENTRE_X, y: PH / 2, scale: 1.0 }, { t: 8, x: P_CENTRE_X, y: PH / 2, scale: 0.985 }],
  badge: { x: SAFE_X / 2 + 12, y: 210, font: 32 },
  card: 1.2,
  nameSize: 22,
  pulse: 8,
};

const linksOf = (nodes: Node[]) => nodes.flatMap((a, i) => nodes.slice(i + 1).map((b, j) => ({ a, b, at: Math.max(a.at, b.at) + 0.55 + ((i * 5 + j) % 7) * 0.12 })));
export const LINKS = { landscape: linksOf(LANDSCAPE_NODES), portrait: linksOf(PORTRAIT_NODES) };
const LINK_DRAW = 0.9;

export const TRAFFIC: Array<[from: number, to: number, start: number]> = [
  [0, 3, 4.2], [1, 4, 4.7], [5, 2, 5.2], [0, 1, 5.6], [3, 5, 6.0], [2, 0, 6.4], [4, 1, 6.6], [0, 4, 6.9], [3, 2, 7.1],
];

function MeshHero({ t }: { t: number }) {
  const portrait = useStage().portrait;
  const layout = portrait ? PORTRAIT : LANDSCAPE;
  const links = portrait ? LINKS.portrait : LINKS.landscape;
  const { nodes, badge: at } = layout;
  const badge = range(t, 4.2, 5.0, easeOut);
  const machines = useGuest() && t >= GUEST_JOINS ? 7 : 6;
  return (
    <div style={{ position: "absolute", inset: 0, background: meshBackground }}>
      <Camera t={t} width={layout.width} height={layout.height} keys={layout.camera}>
        <svg width={layout.width} height={layout.height} style={{ position: "absolute", inset: 0, overflow: "visible" }}>
          {links.map(({ a, b, at: linkAt }) => {
            const k = range(t, linkAt, linkAt + LINK_DRAW);
            if (k <= 0) return null;
            const live = range(t, linkAt + LINK_DRAW, linkAt + LINK_DRAW + 1.2, easeOut);
            return (
              <ConnectionLine key={`${a.peer}-${b.peer}`} fromX={a.x} fromY={a.y} toX={lerp(a.x, b.x, k)} toY={lerp(a.y, b.y, k)} pushCount={Math.round(4 + 3 * (1 - live))} pulsing={false} />
            );
          })}
          {TRAFFIC.map(([from, to, start]) => (
            <Pulse key={`${from}-${to}-${start}`} t={t} from={[nodes[from].x, nodes[from].y]} to={[nodes[to].x, nodes[to].y]} start={start} end={start + 1.5} r={layout.pulse} />
          ))}
        </svg>
        <At x={at.x} y={at.y} z={1}>
          <div data-film-anchor="mesh" style={{ opacity: badge, transform: `scale(${lerp(0.92, 1, badge)})`, display: "flex", alignItems: "center", gap: 14, padding: "16px 30px", borderRadius: 999, border: "1.5px solid color-mix(in srgb, var(--ct-accent) 55%, var(--ct-border))", background: "var(--ct-bg)", boxShadow: "var(--ct-shadow-lg)", color: "var(--ct-fg)", fontSize: at.font, whiteSpace: "nowrap" }}>
            <Lock size={at.font} color="var(--ct-accent)" />
            <span style={{ fontWeight: 600 }}>Private tailnet</span>
            <span style={{ color: "var(--ct-muted-fg)" }}>· {machines} machines · one mesh</span>
          </div>
        </At>
        {nodes.map(({ peer, x, y, at: cardAt }) => (
          <At key={peer} x={x} y={y} z={2}>
            <Appear t={t} at={cardAt}>
              <div data-film-anchor={peer === "laptop" ? "laptop" : undefined}>
                <MachineCard peer={peer} self={peer === "laptop"} scale={layout.card} nameSize={layout.nameSize} />
              </div>
            </Appear>
          </At>
        ))}
      </Camera>
    </div>
  );
}

const meshHero: Scene = {
  id: "mesh-hero",
  duration: 8,
  render: (t) => <MeshHero t={t} />,
};

export default meshHero;
