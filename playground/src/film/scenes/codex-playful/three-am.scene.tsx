import { Bell } from "lucide-react";
import { agentTab } from "../../../gallery/fixtures";
import { range, useStage, type Scene } from "../../engine";
import { AgentToy, Bubble, kick, Set, spring, Toy } from "./toys";

function ThreeAm({ t }: { t: number }) {
  const { portrait } = useStage();
  const awake = t >= 3;
  const poke = kick(t, 3, 0.7);
  const anticipation = awake ? 0 : range(t, 1.8, 3);
  const pip = agentTab("cx-night-pip", { title: "Pip", description: awake ? "You did say first thing" : "Enjoying a quiet moment", agent_state: awake ? { state: "working", status_line: "Checking the overnight render" } : { state: "idle" } });
  return <Set title={<>You asked<br />for a nudge.</>}>
    <Toy x={portrait ? 120 : 70} y={portrait ? 240 : 300} turn={poke * 4} squash={poke * 0.05}>
      <div style={{ fontSize: portrait ? 240 : 330, lineHeight: 1, fontWeight: 700, letterSpacing: "-0.08em", fontVariantNumeric: "tabular-nums", color: awake ? "var(--ct-accent)" : "var(--ct-fg)" }}>{awake ? "03:00" : t < 1.8 ? "02:58" : "02:59"}</div>
    </Toy>
    <Toy x={portrait ? 120 : 970} y={(portrait ? 620 : 300) - poke * (portrait ? 45 : 90) + anticipation * 16} scale={3} turn={awake ? poke * -6 : -1} squash={poke * 0.07 + anticipation * 0.025}>
      <AgentToy tab={pip} width={280} />
    </Toy>
    {awake && <Toy x={portrait ? 120 : 950} y={(portrait ? 1020 : 720) + (1 - spring(t, 3)) * (portrait ? 45 : 100)} scale={2.8} turn={-1 + poke * 3}>
      <Bubble id="cx-night-cron" width={310} text="Check the overnight render on gpu-rig." origin={{ kind: "automation", source: "cron", name: "night" }} />
    </Toy>}
    {t >= 5.5 && <Toy x={portrait ? 120 : 90} y={(portrait ? 1320 : 850) + (1 - spring(t, 5.5)) * (portrait ? 12 : 65)} scale={2.6} turn={1} squash={kick(t, 5.5) * 0.04}>
      <Bubble id="cx-night-joke" width={310} assistant text="You did say “first thing.”" />
    </Toy>}
    {awake && <Toy x={portrait ? 840 : 1690} y={(portrait ? 475 : 100) - poke * 55} turn={poke * 25} squash={poke * 0.08}>
      <Bell size={portrait ? 140 : 160} strokeWidth={1.5} style={{ color: "var(--ct-yellow)" }} />
    </Toy>}
    <div style={{ position: "absolute", left: portrait ? 120 : 85, top: portrait ? 560 : 710, fontFamily: "var(--ct-font-mono, monospace)", color: "var(--ct-muted-fg)", fontSize: 34 }}>0 3 * * * <span style={{ color: "var(--ct-accent)", marginLeft: 22 }}>night</span></div>
  </Set>;
}

export default { id: "cx-playful-three-am", duration: 8, render: (t) => <ThreeAm t={t} /> } satisfies Scene;
