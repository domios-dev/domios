import { ConnectionLine } from "../../../../../src/components/MeshPanel/ConnectionLine";
import { agentTab, CODEX } from "../../../gallery/fixtures";
import { clamp, useStage, type Scene } from "../../engine";
import { AgentToy, Bubble, kick, Set, spring, Toy } from "./toys";

function MeshGossip({ t }: { t: number }) {
  const { portrait, width, height } = useStage();
  const reply = t >= 4.3;
  const pip = agentTab("cx-gossip-pip", { title: "Pip", description: reply ? "Delegation looks good on you" : "Asking gpu-rig for a hand", agent_state: { state: reply ? "idle" : "working", status_line: reply ? undefined : "Sending the render request" } });
  const nudge = agentTab("cx-gossip-nudge", { title: "Nudge", agent_type: CODEX.id, agent_session_model: CODEX.default_model, description: t >= 2.6 ? "Taking the render on gpu-rig" : "Ready for a little gossip", agent_state: { state: t >= 2.6 ? "working" : "idle", status_line: t >= 2.6 ? "Preparing the render" : undefined } });
  const left = portrait ? { x: 134, y: 280 } : { x: 90, y: 300 };
  const right = portrait ? { x: 134, y: 1080 } : { x: 1040, y: 300 };
  const from = portrait ? { x: 540, y: 610 } : { x: 900, y: 470 };
  const to = portrait ? { x: 540, y: 1070 } : { x: 1030, y: 470 };
  const start = reply ? 4.3 : 1.4;
  const travel = clamp((t - start) / 1.2);
  const k = reply ? 1 - travel : travel;
  const arrival = kick(t, 2.6);
  const returned = kick(t, 5.5);
  return <Set title={<>They talk.<br />Work happens.</>}>
    <svg width={width} height={height} style={{ position: "absolute", inset: 0 }}>
      <ConnectionLine fromX={from.x} fromY={from.y} toX={to.x} toY={to.y} pushCount={2} pulsing={false} />
      {travel > 0 && travel < 1 && <ellipse cx={from.x + (to.x - from.x) * k} cy={from.y + (to.y - from.y) * k - (portrait ? 0 : Math.sin(travel * Math.PI) * 85)} rx={portrait ? 12 : 23} ry={portrait ? 23 : 12} fill="var(--ct-accent)" />}
    </svg>
    <Toy x={left.x} y={left.y - returned * 35 + (1 - spring(t, -0.2)) * 130} scale={2.9} turn={-1 + returned * 4} squash={returned * 0.065}><AgentToy tab={pip} width={280} /></Toy>
    <Toy x={right.x} y={right.y - arrival * 35 + (1 - spring(t, 0.15)) * 130} scale={2.9} turn={1 - arrival * 4} squash={arrival * 0.065}><AgentToy tab={nudge} width={280} /></Toy>
    <div style={{ position: "absolute", left: left.x + 8, top: left.y - 55, fontSize: 36, color: "var(--ct-accent)" }}>laptop</div>
    <div style={{ position: "absolute", left: right.x + 8, top: right.y - 55, fontSize: 36, color: "var(--ct-accent)" }}>gpu-rig</div>
    {t >= 1.1 && (!portrait || !reply) && <Toy x={portrait ? 134 : 100} y={(portrait ? 700 : 730) + (1 - spring(t, 1.1)) * 100} scale={portrait ? 2.6 : 2.65} turn={-1} squash={kick(t, 1.1) * 0.04}>
      <Bubble id="cx-gossip-request" text="Psst. Can you take the render?" width={310} origin={{ kind: "tab", title: "Pip", tabIdShort: "c1a2b3d4", mesh: "laptop" }} />
    </Toy>}
    {t >= 4.3 && <Toy x={portrait ? 134 : 1040} y={(portrait ? 700 : 730) + (1 - spring(t, 4.3)) * 100} scale={portrait ? 2.6 : 2.65} turn={1} squash={kick(t, 4.3) * 0.04}>
      <Bubble id="cx-gossip-reply" text="On it. You take the credit." width={310} origin={{ kind: "tab", title: "Nudge", tabIdShort: "d4c3b2a1", mesh: "gpu-rig" }} />
    </Toy>}
  </Set>;
}

export default { id: "cx-playful-mesh-gossip", duration: 8, render: (t) => <MeshGossip t={t} /> } satisfies Scene;
