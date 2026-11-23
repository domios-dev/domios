import { range, useStage, type Scene } from "../../engine";
import { MachineCard, type PeerName } from "../mesh/meshFixtures";
import { accent, Frame, muted, Rule, Slam, Slug } from "./craft";

const peers: PeerName[] = ["laptop", "gpu-rig", "mac-mini", "homelab", "studio", "pi-hub"];

function Ignition({ t }: { t: number }) {
  const { portrait } = useStage();
  const lit = Math.min(6, Math.floor(t / .55) + 1);
  return <Frame>
    <Slug style={{ left: portrait ? 120 : 64, top: portrait ? 100 : 70 }}>Domios / private mesh</Slug>
    <div style={{ position: "absolute", left: portrait ? 115 : 55, top: portrait ? 205 : 245 }}>
      <Slam t={t} at={0} size={portrait ? 245 : 284}>SIX.</Slam>
      <Slam t={t} at={.55} size={portrait ? 111 : 116} style={{ marginTop: 22 }}>MACHINES.</Slam>
      {!portrait && <Slam t={t} at={1.1} size={116} style={{ marginTop: 20, color: accent }}>WITHIN<br />REACH.</Slam>}
    </div>
    {peers.map((peer, i) => {
      const drift = range(t, 0, 7) * 20;
      const x = (portrait ? 320 + (i % 2) * 440 : 985 + (i % 2) * 560) + (i % 2 ? drift : -drift);
      const y = (portrait ? 685 + Math.floor(i / 2) * 260 : 240 + Math.floor(i / 2) * 280) + (Math.floor(i / 2) - 1) * drift;
      const active = i < lit;
      const kick = 1 - range(t, i * .55, i * .55 + .24);
      return <div key={peer} style={{ position: "absolute", left: x, top: y, width: 360, transform: `translate(-50%, -50%) translateY(${active ? -kick * 36 : 0}px)`, filter: active ? "none" : "grayscale(1) brightness(.35)" }}>
        <div style={{ color: active ? accent : muted, fontSize: 27, marginBottom: 16, letterSpacing: ".12em", fontVariantNumeric: "tabular-nums" }}>0{i + 1} / {active ? "ONLINE" : "STANDBY"}</div>
        <MachineCard peer={peer} self={peer === "laptop"} scale={portrait ? 1.02 : 1.22} nameSize={27} glow={active ? .55 : 0} />
      </div>;
    })}
    <Rule t={t} at={3.3} left={portrait ? 120 : 64} top={portrait ? 1350 : 930} width={portrait ? 840 : 620} />
    <div style={{ position: "absolute", left: portrait ? 120 : 64, top: portrait ? 1384 : 962, fontSize: portrait ? 42 : 36, letterSpacing: "-.02em" }}>{portrait ? "Six machines. One private mesh." : "Your machines. Reach them by name."}</div>
    <Slug style={{ left: portrait ? 120 : 64, top: portrait ? 1440 : 1024, fontSize: 26 }}>{lit} / 6 online</Slug>
  </Frame>;
}

export default { id: "cx-epic-ignition", duration: 7, render: (t) => <Ignition t={t} /> } satisfies Scene;
