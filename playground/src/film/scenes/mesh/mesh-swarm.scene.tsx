import { Check, Download, HardDrive, LoaderCircle, Lock } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { ConnectionLine } from "../../../../../src/components/MeshPanel/ConnectionLine";
import { Camera, easeInOut, easeOut, lerp, range, useStage, type Scene } from "../../engine";
import { useGuest } from "../../guest";
import { LANDSCAPE as HERO_LANDSCAPE, LINKS, PORTRAIT as HERO_PORTRAIT, TRAFFIC, type Node } from "./mesh-hero.scene";
import { At, MachineCard, meshBackground, Pulse, Ripple, Typed, type PeerName, type Resource } from "./meshFixtures";

// Picks up where mesh-hero ends (its t = 8): same ring, links and camera, then two machines step forward and work.
const HERO_END = 8;
const FOCUS = [0.2, 1.8] as const;
const SEND = [3.0, 4.0] as const;
const INSTALL = [4.2, 6.2] as const;
const PULL = [4.8, 8.0] as const;
const READY = 8.1;
const ACK = [8.4, 9.4] as const;
const STUDIO = [10.4, 11.8] as const;
const FARM = [12.6, 13.8] as const;
const RENDER = [14.8, 15.6] as const;
const DURATION = 16.6;
const MODEL_GB = 6.9;
const COMMAND = `domios send --mesh gpu-rig "Install SwarmUI and pull SDXL"`;

type Point = [number, number];
type Layout = {
  hero: typeof HERO_LANDSCAPE;
  links: typeof LINKS.landscape;
  laptop: Point;
  rig: Point;
  studio: Point;
  command: Point;
  commandFont: number;
  panel: Point;
  report: Point;
  farm: { x: number; y: number; w: number; h: number };
  card: number;
  ui: number;
};

const LANDSCAPE: Layout = {
  hero: HERO_LANDSCAPE,
  links: LINKS.landscape,
  laptop: [470, 400],
  rig: [1450, 400],
  studio: [960, 700],
  command: [960, 150],
  commandFont: 24,
  panel: [1450, 650],
  report: [470, 620],
  farm: { x: 200, y: 215, w: 1520, h: 660 },
  card: 1.42,
  ui: 1,
};

const PORTRAIT: Layout = {
  hero: HERO_PORTRAIT,
  links: LINKS.portrait,
  laptop: [540, 500],
  rig: [540, 1040],
  studio: [540, 1660],
  command: [540, 250],
  commandFont: 24,
  panel: [540, 1360],
  report: [540, 780],
  farm: { x: 70, y: 330, w: 940, h: 1560 },
  card: 1.9,
  ui: 1.6,
};

const ACTIVE: PeerName[] = ["laptop", "gpu-rig", "studio"];

function Bar({ k }: { k: number }) {
  return (
    <div style={{ height: 8, borderRadius: 4, background: "color-mix(in srgb, var(--ct-fg) 12%, transparent)", overflow: "hidden" }}>
      <div style={{ width: `${k * 100}%`, height: "100%", borderRadius: 4, background: "var(--ct-accent)" }} />
    </div>
  );
}

function Step({ icon, label, detail, k, done }: { icon: ReactNode; label: string; detail?: string; k: number; done: boolean }) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "28px 1fr", columnGap: 12, rowGap: 8, alignItems: "center" }}>
      <span style={{ display: "grid", placeItems: "center", color: done ? "var(--ct-green)" : "var(--ct-accent)" }}>{done ? <Check size={22} /> : icon}</span>
      <div style={{ display: "flex", justifyContent: "space-between", gap: 16, fontSize: 20, color: "var(--ct-fg)" }}>
        <span>{label}</span>
        {detail && <span style={{ fontFamily: "var(--ct-font-mono, ui-monospace, monospace)", color: "var(--ct-muted-fg)" }}>{detail}</span>}
      </div>
      <span />
      <Bar k={k} />
    </div>
  );
}

const panelStyle = (scale: number, opacity: number, rise: number): CSSProperties => ({
  width: 440, padding: "20px 22px", display: "flex", flexDirection: "column", gap: 18, borderRadius: 14, border: "1px solid var(--ct-border)",
  background: "var(--ct-bg)", boxShadow: "var(--ct-shadow-lg)", opacity, transform: `translateY(${(1 - rise) * 14}px) scale(${scale})`,
});

function InstallPanel({ t, scale }: { t: number; scale: number }) {
  const show = range(t, INSTALL[0] - 0.2, INSTALL[0] + 0.3, easeOut) * (1 - range(t, STUDIO[0], STUDIO[0] + 0.5));
  if (show <= 0) return null;
  const install = range(t, INSTALL[0], INSTALL[1], easeInOut);
  const pull = range(t, PULL[0], PULL[1], easeInOut);
  return (
    <div style={panelStyle(scale, show, show)}>
      <div style={{ fontSize: 18, color: "var(--ct-muted-fg)" }}>gpu-rig · Orbit</div>
      <Step icon={<LoaderCircle size={22} style={{ transform: `rotate(${t * 300}deg)` }} />} label="Installing SwarmUI" k={install} done={install >= 1} />
      <Step icon={<Download size={22} />} label="sdxl-base-1.0" detail={`${(pull * MODEL_GB).toFixed(1)} / ${MODEL_GB} GB`} k={pull} done={pull >= 1} />
    </div>
  );
}

function ReportChip({ t, scale }: { t: number; scale: number }) {
  const show = range(t, ACK[1], ACK[1] + 0.4, easeOut) * (1 - range(t, STUDIO[0], STUDIO[0] + 0.5));
  if (show <= 0) return null;
  return (
    <div style={{ ...panelStyle(scale, show, show), width: 440, gap: 6 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 20, color: "var(--ct-fg)" }}>
        <Check size={22} color="var(--ct-green)" /> SwarmUI ready on gpu-rig
      </div>
      <div style={{ fontSize: 18, color: "var(--ct-muted-fg)", paddingLeft: 32 }}>sdxl-base-1.0 loaded · gpu-rig:7801</div>
    </div>
  );
}

function FarmFrame({ t, farm }: { t: number; farm: Layout["farm"] }) {
  const draw = range(t, FARM[0], FARM[1], easeInOut);
  if (draw <= 0) return null;
  const label = range(t, FARM[1] - 0.3, FARM[1] + 0.4, easeOut);
  const perimeter = 2 * (farm.w + farm.h);
  return (
    <>
      <svg style={{ position: "absolute", left: farm.x, top: farm.y, width: farm.w, height: farm.h, overflow: "visible" }} aria-hidden="true">
        <rect x={0} y={0} width={farm.w} height={farm.h} rx={28} fill="color-mix(in srgb, var(--ct-accent) 4%, transparent)" stroke="var(--ct-accent)" strokeWidth={2} strokeDasharray={`${perimeter * draw} ${perimeter}`} opacity={0.75} />
      </svg>
      <div style={{ position: "absolute", left: farm.x + 28, top: farm.y - 26, opacity: label, transform: `translateY(${(1 - label) * 8}px)`, display: "flex", alignItems: "center", gap: 12, padding: "10px 20px", borderRadius: 999, background: "var(--ct-bg)", border: "1.5px solid color-mix(in srgb, var(--ct-accent) 60%, var(--ct-border))", fontSize: 24, color: "var(--ct-fg)", whiteSpace: "nowrap" }}>
        <span style={{ fontWeight: 600 }}>Render farm</span>
        <span style={{ color: "var(--ct-muted-fg)" }}>laptop directs · gpu-rig renders · studio stores</span>
      </div>
    </>
  );
}

function RenderChip({ t, at, scale }: { t: number; at: Point; scale: number }) {
  const show = range(t, RENDER[1], RENDER[1] + 0.4, easeOut);
  if (show <= 0) return null;
  return (
    <At x={at[0]} y={at[1] + 140} z={3}>
      <div style={{ ...panelStyle(scale, show, show), width: "auto", flexDirection: "row", alignItems: "center", gap: 12, padding: "12px 18px" }}>
        <HardDrive size={22} color="var(--ct-accent)" />
        <span style={{ fontSize: 20, color: "var(--ct-fg)", whiteSpace: "nowrap" }}>shot-14 / frame_0001.png · 4K</span>
      </div>
    </At>
  );
}

function MeshSwarm({ t }: { t: number }) {
  const { portrait } = useStage();
  const L = portrait ? PORTRAIT : LANDSCAPE;
  const nodes = L.hero.nodes;
  const camera = L.hero.camera[L.hero.camera.length - 1];
  const focus = range(t, FOCUS[0], FOCUS[1], easeInOut);
  const studio = range(t, STUDIO[0], STUDIO[1], easeInOut);
  const badge = 1 - range(t, 0, 0.8);
  const guest = useGuest();
  const command = range(t, 1.1, 1.5, easeOut) * (1 - range(t, STUDIO[0], STUDIO[0] + 0.5));
  const target: Partial<Record<PeerName, [Point, number]>> = { laptop: [L.laptop, focus], "gpu-rig": [L.rig, focus], studio: [L.studio, studio] };
  const place = ({ peer, x, y }: Node): Point => {
    const goal = target[peer];
    if (goal && goal[1] > 0) return [lerp(x, goal[0][0], goal[1]), lerp(y, goal[0][1], goal[1])];
    if (ACTIVE.includes(peer)) return [x, y];
    const k = 0.7 * focus;
    return [lerp(x, camera.x ?? L.hero.width / 2, -k), lerp(y, camera.y ?? L.hero.height / 2, -k)];
  };
  const dimOf = (peer: PeerName) => (peer === "laptop" || peer === "gpu-rig" ? 1 : peer === "studio" ? lerp(lerp(1, 0.04, focus), 1, studio) : lerp(1, 0, focus));
  const at = Object.fromEntries(nodes.map((n) => [n.peer, place(n)])) as Record<PeerName, Point>;
  const swarmui: Resource[] = t >= INSTALL[0] + 0.1 ? [["swarmui", "daemon", t >= READY ? undefined : "degraded"]] : [];
  // Studio's role shows on its card: a storage service comes up as it joins.
  const storage: Resource[] = t >= STUDIO[0] + 0.5 ? [["minio", "daemon", t >= STUDIO[1] + 0.4 ? undefined : "degraded"]] : [];
  const lit = (a: PeerName, b: PeerName) => {
    if ((a === "laptop" && b === "gpu-rig") || (a === "gpu-rig" && b === "laptop")) return 1;
    if (ACTIVE.includes(a) && ACTIVE.includes(b)) return Math.max(lerp(1, 0.12, focus), studio);
    return lerp(1, 0.08, focus);
  };

  return (
    <div style={{ position: "absolute", inset: 0, background: meshBackground }}>
      <Camera t={t} width={L.hero.width} height={L.hero.height} keys={[{ t: 0, x: camera.x, y: camera.y, scale: camera.scale ?? 1 }, { t: DURATION, x: camera.x, y: camera.y, scale: (camera.scale ?? 1) * 1.02 }]}>
        <FarmFrame t={t} farm={L.farm} />
        <svg width={L.hero.width} height={L.hero.height} style={{ position: "absolute", inset: 0, overflow: "visible" }}>
          {L.links.map(({ a, b }) => (
            <g key={`${a.peer}-${b.peer}`} opacity={lit(a.peer, b.peer)}>
              <ConnectionLine fromX={at[a.peer][0]} fromY={at[a.peer][1]} toX={at[b.peer][0]} toY={at[b.peer][1]} pushCount={a.peer === "laptop" && b.peer === "gpu-rig" && t >= SEND[0] && t < ACK[1] ? 6 : 3} pulsing={false} />
            </g>
          ))}
          {TRAFFIC.map(([from, to, start]) => (
            <Pulse key={`${from}-${to}-${start}`} t={t + HERO_END} from={at[nodes[from].peer]} to={at[nodes[to].peer]} start={start} end={start + 1.5} r={L.hero.pulse} />
          ))}
          <Pulse t={t} from={at.laptop} to={at["gpu-rig"]} start={SEND[0]} end={SEND[1]} r={L.hero.pulse * 1.2} />
          <Pulse t={t} from={at["gpu-rig"]} to={at.laptop} start={ACK[0]} end={ACK[1]} r={L.hero.pulse * 1.5} />
          <Pulse t={t} from={at["gpu-rig"]} to={at.studio} start={RENDER[0]} end={RENDER[1]} r={L.hero.pulse} />
        </svg>
        {badge > 0 && (
          <At x={L.hero.badge.x} y={L.hero.badge.y} z={1}>
            <div style={{ opacity: badge, display: "flex", alignItems: "center", gap: 14, padding: "16px 30px", borderRadius: 999, border: "1.5px solid color-mix(in srgb, var(--ct-accent) 55%, var(--ct-border))", background: "var(--ct-bg)", boxShadow: "var(--ct-shadow-lg)", color: "var(--ct-fg)", fontSize: L.hero.badge.font, whiteSpace: "nowrap" }}>
              <Lock size={L.hero.badge.font} color="var(--ct-accent)" />
              <span style={{ fontWeight: 600 }}>Private tailnet</span>
              <span style={{ color: "var(--ct-muted-fg)" }}>· {guest ? 7 : 6} machines · one mesh</span>
            </div>
          </At>
        )}
        {command > 0 && (
          <At x={L.command[0]} y={L.command[1]} z={3}>
            <div style={{ opacity: command, display: "flex", gap: 14, padding: "14px 22px", borderRadius: 12, border: "1px solid var(--ct-border)", background: "var(--ct-bg)", boxShadow: "var(--ct-shadow-lg)", fontFamily: "var(--ct-font-mono, ui-monospace, monospace)", fontSize: L.commandFont, color: "var(--ct-fg)", whiteSpace: "pre" }}>
              <span style={{ color: "var(--ct-accent)" }}>laptop ›</span>
              <Typed t={t} at={1.3} cps={48} text={COMMAND} />
            </div>
          </At>
        )}
        {nodes.map((node) => (
          <At key={node.peer} x={at[node.peer][0]} y={at[node.peer][1]} z={ACTIVE.includes(node.peer) ? 2 : 1} style={{ opacity: dimOf(node.peer) }}>
            <MachineCard peer={node.peer} self={node.peer === "laptop"} scale={L.card} nameSize={portrait ? 22 : 19} glow={node.peer === "gpu-rig" ? range(t, SEND[1], SEND[1] + 0.3) * (1 - range(t, READY, READY + 0.8)) : 0} extra={node.peer === "gpu-rig" ? swarmui : node.peer === "studio" ? storage : []} />
          </At>
        ))}
        <svg width={L.hero.width} height={L.hero.height} style={{ position: "absolute", inset: 0, overflow: "visible", zIndex: 4, pointerEvents: "none" }}>
          <Ripple t={t} x={at["gpu-rig"][0]} y={at["gpu-rig"][1]} at={SEND[1]} size={90} />
          <Ripple t={t} x={at.laptop[0]} y={at.laptop[1]} at={ACK[1]} size={70} />
          <Ripple t={t} x={at.studio[0]} y={at.studio[1]} at={RENDER[1]} size={70} />
        </svg>
        <At x={L.panel[0]} y={L.panel[1]} z={3}>
          <InstallPanel t={t} scale={L.ui} />
        </At>
        <At x={L.report[0]} y={L.report[1]} z={3}>
          <ReportChip t={t} scale={L.ui} />
        </At>
        <RenderChip t={t} at={at.studio} scale={L.ui} />
      </Camera>
    </div>
  );
}

const meshSwarm: Scene = {
  id: "mesh-swarm",
  duration: DURATION,
  render: (t) => <MeshSwarm t={t} />,
};

export default meshSwarm;
