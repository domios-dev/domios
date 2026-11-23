import type { ReactNode } from "react";
import { PeerTerminal } from "../../../../../src/components/MeshPanel/PeerTerminal";
import { peerInventoryModel } from "../../../../../src/components/MeshPanel/peerInventoryModel";
import { ConnectionLine } from "../../../../../src/components/MeshPanel/ConnectionLine";
import type { Service } from "../../../../../src/components/MeshPanel/types";
import { TabItem } from "../../../../../src/components/Sidebar/TabItem";
import { agentTab, ProductProviders, WORKSPACE_ID } from "../../../gallery/fixtures";
import { range, Reveal, sequence, useStage, type Scene } from "../../engine";

const MACHINES = ["laptop", "gpu-rig", "mac-mini", "homelab", "studio", "pi-hub"] as const;
type Machine = typeof MACHINES[number];
const SCALE = 2.4;
const noop = () => undefined;

const light = (t: number, at: number, duration = 1) => range(t, at, at + 0.25) * (1 - range(t, at + duration - 0.25, at + duration));
const arrivalPulse = (t: number, at: number) => Math.sin(Math.PI * range(t, at, at + 0.5));

function Frame({ t, children, arrivals = false }: { t: number; children: ReactNode; arrivals?: boolean }) {
  const { portrait } = useStage();
  const push = range(t, 0, 6);
  return <div className="mob-pulse" style={{ position: "absolute", inset: 0, background: "radial-gradient(ellipse at 47% 42%, color-mix(in srgb, var(--ct-accent) 5%, var(--ct-bg)), var(--ct-bg) 65%)", overflow: "hidden" }}>
    <style>{`
      .mob-pulse .pulse-ui [class*="text-"] { font-size: 13px; }
      .mob-pulse .pulse-ui .font-medium { font-size: 15px; }
      .mob-pulse .pulse-ui * { transition: none !important; }
      .mob-pulse .activity-dots i { animation: none !important; }
      .mob-pulse .pulse-surface > div > [role="group"] {
        background: linear-gradient(145deg, color-mix(in srgb, var(--ct-titlebar-bg) 65%, var(--ct-bg)), var(--ct-bg));
        box-shadow: 0 9px 24px color-mix(in srgb, var(--ct-fg) 10%, transparent), 0 1px 2px color-mix(in srgb, var(--ct-fg) 12%, transparent);
      }
      .mob-pulse .gallery-sidebar { max-width: none; margin: 0; }
    `}</style>
    <div style={{ position: "absolute", left: portrait ? "calc(50% - 504px)" : "calc(50% - 540px)", top: 0, width: 1080, height: 1920, transformOrigin: "504px 810px", transform: arrivals ? `translateY(${-8 * push}px) scale(${1 + push * 0.018})` : `translate(${6 - 12 * push}px, ${-17 + 34 * push}px)` }}>{children}</div>
  </div>;
}

function Placement({ y, children, t, at = -0.25, active = 0, pulse = 0 }: { y: number; children: ReactNode; t: number; at?: number; active?: number; pulse?: number }) {
  return <div style={{ position: "absolute", left: 72, top: y, width: 864 }}>
    <Reveal t={t} at={at} dur={0.25} dy={24}>
      <div className="pulse-ui" style={{ width: 360, transform: `scale(${SCALE})`, transformOrigin: "0 0", opacity: 0.82 + active * 0.18 }}>
        <div className="pulse-surface" style={{ position: "relative", borderRadius: 8, transform: `scale(${1 + pulse * 0.009})` }}>
          <div style={{ position: "absolute", inset: -10, borderRadius: 16, background: "radial-gradient(ellipse at 50% 75%, color-mix(in srgb, var(--ct-accent) 36%, transparent), transparent 75%)", filter: "blur(16px)", opacity: active * 0.7 + pulse * 0.2, pointerEvents: "none" }} />
          <div style={{ position: "relative" }}>{children}</div>
        </div>
      </div>
    </Reveal>
  </div>;
}

function inventory(peer: Machine, live: boolean) {
  const service: Service = {
    id: `mob-pulse-${peer}-render`, name: "render", kind: "daemon",
    health_mode: "process", health_interval: 30, health: { status: live ? "healthy" : "unknown" },
    push_enabled: true, registered_at: 1, updated_at: 1, enabled: true,
    source_peer_id: peer, source_peer: peer,
  };
  return peerInventoryModel([service], { instance_id: peer, name: peer, online: true, services_loaded: true });
}

function MachineCard({ peer, live }: { peer: Machine; live?: boolean }) {
  return <PeerTerminal name={peer} ip="" isThisMachine={peer === "laptop"} isPaired={peer !== "laptop"}
    inventory={live === undefined ? undefined : inventory(peer, live)} />;
}

function AgentRow({ working, description }: { working: boolean; description: string }) {
  const tab = agentTab("mob-pulse-render-agent", {
    title: "Render", description,
    agent_state: working ? { state: "working", status_line: description } : { state: "idle" },
  });
  return <ProductProviders tabs={[tab]}>
    <div className="gallery-sidebar" style={{ width: 360, background: "linear-gradient(145deg, var(--ct-titlebar-bg), var(--ct-sidebar-bg))", border: "1px solid var(--ct-border)", borderRadius: 6, boxShadow: "0 9px 24px color-mix(in srgb, var(--ct-fg) 10%, transparent)" }}>
      <TabItem tab={tab} index={0} isActive={working} onClick={noop} onClose={noop} canClose={false}
        workspaceId={WORKSPACE_ID} hasAgent status={working ? "Working" : undefined} />
    </div>
  </ProductProviders>;
}

function Relay({ t, returning = true }: { t: number; returning?: boolean }) {
  const outgoing = t >= 1.5 && t < 2.5;
  const incoming = returning && t >= 3.5 && t < 4.5;
  const k = outgoing ? range(t, 1.5, 2.5) : incoming ? 1 - range(t, 3.5, 4.5) : 0;
  const y = 560 + 550 * k;
  const tail = outgoing ? Math.max(560, y - 70) : Math.min(1110, y + 70);
  return <svg width={1080} height={1480} style={{ position: "absolute", inset: 0 }}>
    <ConnectionLine fromX={504} fromY={560} toX={504} toY={1110} pushCount={outgoing || incoming ? 3 : 0} pulsing={false} />
    {(outgoing || incoming) && <g>
      <line x1={504} x2={504} y1={tail} y2={y} stroke="var(--ct-accent)" strokeWidth={12} strokeLinecap="round" opacity={0.2} style={{ filter: "blur(5px)" }} />
      <circle cx={504} cy={y} r={22} fill="var(--ct-accent)" opacity={0.16} style={{ filter: "blur(6px)" }} />
      <circle cx={504} cy={y} r={11} fill="var(--ct-accent)" />
    </g>}
  </svg>;
}

const arrivals: Scene = {
  id: "mob-pulse-arrivals", duration: 6,
  render: (t) => <Frame t={t} arrivals>{MACHINES.map((peer, i) => <Placement key={peer} y={170 + i * 225} t={t} at={i === 0 ? -0.25 : i * 0.5} active={light(t, i === 0 ? -0.25 : i * 0.5, 0.75)}>
    <MachineCard peer={peer} />
  </Placement>)}</Frame>,
};

const live: Scene = {
  id: "mob-pulse-live", duration: 6,
  render: (t) => <Frame t={t}>{(["gpu-rig", "homelab", "studio"] as const).map((peer, i) => <Placement key={peer} y={180 + i * 440} t={t} active={light(t, 1 + i * 0.5, 1.5)} pulse={arrivalPulse(t, 1 + i * 0.5)}>
    <MachineCard peer={peer} live={t >= 1 + i * 0.5} />
  </Placement>)}</Frame>,
};

const roundtrip: Scene = {
  id: "mob-pulse-roundtrip", duration: 6,
  render: (t) => <Frame t={t}>
    <Relay t={t} />
    <Placement y={220} t={t} active={Math.max(light(t, 1.5), light(t, 4.5))} pulse={arrivalPulse(t, 4.5)}><MachineCard peer="laptop" /></Placement>
    <Placement y={410} t={t} active={Math.max(light(t, 1.5), light(t, 4.5))}><AgentRow key={t >= 4.5 ? "returned" : "request"} working={t >= 1.5 && t < 4.5} description={t >= 4.5 ? "Render ready" : "Check the render"} /></Placement>
    <Placement y={1110} t={t} active={light(t, 2.5, 2)} pulse={arrivalPulse(t, 2.5)}><MachineCard peer="gpu-rig" /></Placement>
    <Placement y={1300} t={t} at={2.5} active={light(t, 2.5, 2)}><AgentRow key={t >= 3.5 ? "result" : "received"} working={t < 3.5} description={t >= 3.5 ? "Render ready" : "Checking render"} /></Placement>
  </Frame>,
};

const remote: Scene = {
  id: "mob-pulse-remote", duration: 6,
  render: (t) => <Frame t={t}>
    <Relay t={t} returning={false} />
    <Placement y={220} t={t} active={light(t, 1.5)}><MachineCard peer="laptop" /></Placement>
    <Placement y={410} t={t} active={light(t, 1.5)}><AgentRow working={false} description="Start an agent on gpu-rig" /></Placement>
    <Placement y={1110} t={t} active={light(t, 2.5, 3.5)} pulse={arrivalPulse(t, 2.5)}><MachineCard peer="gpu-rig" /></Placement>
    <Placement y={1300} t={t} at={2.5} active={light(t, 2.5, 3.5)}><AgentRow working description="Rendering the next shot" /></Placement>
  </Frame>,
};

export default [arrivals, live, roundtrip, remote, sequence("mob-pulse-reel", [arrivals, live, roundtrip, remote])];
