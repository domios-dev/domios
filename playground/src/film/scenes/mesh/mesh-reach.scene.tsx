import type { CSSProperties } from "react";
import { TabItem } from "../../../../../src/components/Sidebar/TabItem";
import type { Tab, TabStatus } from "../../../../../src/lib/types";
import { ConnectionLine } from "../../../../../src/components/MeshPanel/ConnectionLine";
import { agentTab, CODEX, ProductProviders, shellTab, WORKSPACE_ID } from "../../../gallery/fixtures";
import { Camera, easeOut, range, Spotlight, track, useStage, type FocusKey, type Key, type Scene } from "../../engine";
import { Appear, At, H, MachineCard, meshBackground, PH, Pulse, PW, Ripple, Typed, W } from "./meshFixtures";

const SIDEBAR_WIDTH = 280;

const SEND_AT = 1.6;
const LAND_AT = 3.0;
const ACK_AT = 3.7;
const ACK_LAND = 4.7;
const TASK = "Re-render shot 14 at 4K";
const COMMAND = `domios send --mesh gpu-rig --tab 7c2e91ab "${TASK}"`;

type Point = [number, number];
type Placement = { x: number; y: number; sidebarTop: number; sidebarAbove?: boolean };
type Layout = {
  width: number;
  height: number;
  camera: Key[];
  laptop: Placement;
  rig: Placement;
  card: number;
  nameSize?: number;
  sidebar: number;
  linkFrom: Point;
  linkTo: Point;
  command: { x: number; y: number; font: number; text: string; width?: number };
  pulse: number;
};

const L_AIR = { x: 560, y: 440 };
const L_ROG = { x: 1360, y: 440 };
const L_CARD_HALF = 207;

const LANDSCAPE: Layout = {
  width: W,
  height: H,
  camera: [{ t: 0, x: 960, y: 505, scale: 1.38 }, { t: LAND_AT, x: 966, y: 510, scale: 1.41 }, { t: 6, x: 970, y: 511, scale: 1.42 }],
  laptop: { ...L_AIR, sidebarTop: 570 },
  rig: { ...L_ROG, sidebarTop: 570 },
  card: 1.15,
  sidebar: 1.5,
  linkFrom: [L_AIR.x + L_CARD_HALF, L_AIR.y],
  linkTo: [L_ROG.x - L_CARD_HALF, L_ROG.y],
  command: { x: W / 2, y: 250, font: 22, text: COMMAND },
  pulse: 9,
};

const P_X = PW / 2;
const P_AIR_Y = 730;
const P_ROG_Y = 1150;

const PORTRAIT: Layout = {
  width: PW,
  height: PH,
  camera: [{ t: 0, x: PW / 2, y: PH / 2, scale: 1.0 }, { t: LAND_AT, x: PW / 2, y: PH / 2 + 5, scale: 1.02 }, { t: 6, x: PW / 2, y: PH / 2 + 10, scale: 1.03 }],
  laptop: { x: P_X, y: P_AIR_Y, sidebarTop: P_AIR_Y - 130, sidebarAbove: true },
  rig: { x: P_X, y: P_ROG_Y, sidebarTop: P_ROG_Y + 152 },
  card: 1.5,
  nameSize: 18,
  sidebar: 2.0,
  linkFrom: [P_X, P_AIR_Y + 110],
  linkTo: [P_X, P_ROG_Y - 125],
  command: { x: P_X, y: 220, font: 28, width: 880, text: COMMAND.replace(` "`, `\n"`) },
  pulse: 11,
};

type Row = { tab: Tab; isActive?: boolean; status?: TabStatus };

function Sidebar({ rows }: { rows: Row[] }) {
  const noop = () => undefined;
  return (
    <ProductProviders tabs={rows.map((row) => row.tab)}>
      <div className="gallery-sidebar" style={{ width: SIDEBAR_WIDTH }}>
        {rows.map((row) => (
          <TabItem key={row.tab.id} index={0} isActive={row.isActive ?? false} onClick={noop} onClose={noop} canClose workspaceId={WORKSPACE_ID} hasAgent={Boolean(row.tab.agent_type)} tab={row.tab} status={row.status} />
        ))}
      </div>
    </ProductProviders>
  );
}

function airRows(t: number): Row[] {
  const sending = t >= 0.4 && t < ACK_LAND;
  const rows: Row[] = [
    { tab: agentTab("air-captain", { title: "Release captain", description: t >= ACK_LAND ? "gpu-rig accepted the 4K pass" : "Handing the 4K pass to gpu-rig", agent_state: sending ? { state: "working", status_line: "Handing the 4K pass to ROG…" } : { state: "idle" } }), isActive: true, status: sending ? "Working" : undefined },
    { tab: shellTab("air-vite", { title: "vite", purpose: "dev", shell_command_state: { activity: "running", last_exit_code: null } }) },
  ];
  if (t >= ACK_LAND) {
    rows.push({ tab: agentTab("mesh:rog/7c2e91ab", { title: "Render farm", remote_peer_id: "rog", remote_peer_name: "gpu-rig", remote_tab_id: "7c2e91ab", agent_type: CODEX.id, agent_session_model: "gpt-6-sol", agent_state: { state: "working", status_line: TASK } }), status: "Working" });
  }
  return rows;
}

function rogRows(t: number): Row[] {
  const landed = t >= LAND_AT;
  return [
    { tab: shellTab("rog-comfy", { title: "comfyui", purpose: "dev", shell_command_state: { activity: "running", last_exit_code: null } }) },
    { tab: agentTab("rog-render", { title: "Render farm", description: landed ? `${TASK} · from laptop` : "Waiting for work", agent_type: CODEX.id, agent_session_model: "gpt-6-sol", agent_state: landed ? { state: "working", status_line: `${TASK} · from laptop` } : { state: "idle" } }), isActive: landed, status: landed ? "Working" : undefined },
  ];
}

const ROW_TOP = 40;
const ROW_HEIGHT = 46;
const FOCUS_IN = LAND_AT + 0.15;
const FOCUS_OUT = ACK_AT + 0.4;

/** Lights the gpu-rig row the task landed on, mapped from world to stage pixels through the camera at each key. */
function landingFocus(layout: Layout): FocusKey[] {
  const { rig, sidebar: s } = layout;
  const world = { x: rig.x - (SIDEBAR_WIDTH / 2) * s - 8, y: rig.sidebarTop + ROW_TOP * s - 6, w: SIDEBAR_WIDTH * s + 16, h: ROW_HEIGHT * s + 12 };
  const at = (t: number) => {
    const c = track(t, layout.camera);
    return { x: (world.x - c.x) * c.scale + layout.width / 2, y: (world.y - c.y) * c.scale + layout.height / 2, w: world.w * c.scale, h: world.h * c.scale };
  };
  return [{ t: FOCUS_IN }, { t: FOCUS_IN + 0.5, rect: at(FOCUS_IN + 0.5) }, { t: FOCUS_OUT, rect: at(FOCUS_OUT) }, { t: FOCUS_OUT + 0.5 }];
}

const FOCUS = { landscape: landingFocus(LANDSCAPE), portrait: landingFocus(PORTRAIT) };

const LANDING_CSS = `.film-landing .gallery-sidebar > .sidebar-tab:nth-child(2) {
  border-radius: 6px;
  box-shadow: 0 0 0 1.5px color-mix(in srgb, var(--ct-accent) calc(var(--landing) * 100%), transparent), 0 0 22px color-mix(in srgb, var(--ct-accent) calc(var(--landing) * 45%), transparent);
}`;

function Machine({ t, peer, at, place, layout, rows, self, landing = 0 }: { t: number; peer: "laptop" | "gpu-rig"; at: number; place: Placement; layout: Layout; rows: Row[]; self?: boolean; landing?: number }) {
  const scale = layout.sidebar;
  const sidebarStyle: CSSProperties = {
    position: "absolute",
    left: place.x - (SIDEBAR_WIDTH / 2) * scale,
    top: place.sidebarTop,
    width: SIDEBAR_WIDTH,
    transform: place.sidebarAbove ? `scale(${scale}) translateY(-100%)` : `scale(${scale})`,
    transformOrigin: "0 0",
    zIndex: 2,
    ["--landing" as string]: landing,
  };
  return (
    <>
      <At x={place.x} y={place.y} z={2}>
        <Appear t={t} at={at}>
          <MachineCard peer={peer} self={self} scale={layout.card} nameSize={layout.nameSize} />
        </Appear>
      </At>
      <div className="film-landing" style={sidebarStyle}>
        <Appear t={t} at={at + 0.2}>
          <Sidebar rows={rows} />
        </Appear>
      </div>
    </>
  );
}

function MeshReach({ t }: { t: number }) {
  const portrait = useStage().portrait;
  const layout = portrait ? PORTRAIT : LANDSCAPE;
  const { linkFrom, linkTo, command: cmd } = layout;
  const command = range(t, -0.2, 0.2, easeOut);
  const glow = range(t, LAND_AT, LAND_AT + 0.4, easeOut) * (1 - range(t, LAND_AT + 1.6, LAND_AT + 2.6));
  return (
    <div style={{ position: "absolute", inset: 0, background: meshBackground }}>
      <Camera t={t} width={layout.width} height={layout.height} keys={layout.camera}>
        <At x={cmd.x} y={cmd.y} z={3}>
          <div style={{ opacity: command, display: "flex", alignItems: "center", flexWrap: cmd.width ? "wrap" : undefined, gap: 14, width: cmd.width, boxSizing: "border-box", padding: cmd.width ? "18px 26px" : "14px 22px", borderRadius: 12, border: "1px solid var(--ct-border)", background: "var(--ct-bg)", boxShadow: "var(--ct-shadow-lg)", fontFamily: "var(--ct-font-mono, ui-monospace, monospace)", fontSize: cmd.font, lineHeight: cmd.width ? 1.45 : undefined, color: "var(--ct-fg)", whiteSpace: cmd.width ? "pre-wrap" : "pre" }}>
            <span style={{ color: "var(--ct-accent)" }}>laptop ›</span>
            <Typed t={t} at={0.45} cps={48} text={cmd.text} />
          </div>
        </At>
        <svg width={layout.width} height={layout.height} style={{ position: "absolute", inset: 0, overflow: "visible", zIndex: 1 }}>
          <ConnectionLine fromX={linkFrom[0]} fromY={linkFrom[1]} toX={linkTo[0]} toY={linkTo[1]} pushCount={t >= SEND_AT && t < ACK_LAND ? 6 : 2} pulsing={false} />
          <Pulse t={t} from={linkFrom} to={linkTo} start={SEND_AT} end={LAND_AT} r={layout.pulse} />
          <Pulse t={t} from={linkTo} to={linkFrom} start={ACK_AT} end={ACK_LAND} r={layout.pulse * 0.67} />
        </svg>
        <Machine t={t} peer="laptop" at={-0.4} place={layout.laptop} layout={layout} rows={airRows(t)} self />
        <Machine t={t} peer="gpu-rig" at={-0.25} place={layout.rig} layout={layout} rows={rogRows(t)} landing={glow} />
        <style>{LANDING_CSS}</style>
        <svg width={layout.width} height={layout.height} style={{ position: "absolute", inset: 0, overflow: "visible", zIndex: 4, pointerEvents: "none" }}>
          <Ripple t={t} x={linkTo[0]} y={linkTo[1]} at={LAND_AT} size={80} />
          <Ripple t={t} x={linkFrom[0]} y={linkFrom[1]} at={ACK_LAND} size={50} />
        </svg>
      </Camera>
      <Spotlight t={t} keys={portrait ? FOCUS.portrait : FOCUS.landscape} radius={10} />
    </div>
  );
}

const meshReach: Scene = {
  id: "mesh-reach",
  duration: 6,
  render: (t) => <MeshReach t={t} />,
};

export default meshReach;
