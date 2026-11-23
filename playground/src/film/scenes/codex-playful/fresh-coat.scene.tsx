import type { CSSProperties } from "react";
import { PaintRoller } from "lucide-react";
import { themeToCSS } from "../../../../../src/lib/themeCss";
import { PLAN, PLAN_INDEX } from "../../fixtures/plan";
import { MEMBERS, TeamSidebar } from "../../fixtures/team";
import { StatusRow } from "../../fixtures/status";
import { SOLARIZED_LIGHT } from "../../fixtures/themes";
import { range, useStage, type Scene } from "../../engine";
import { Bubble, kick, Set, spring, Toy } from "./toys";

const light = { ...themeToCSS(SOLARIZED_LIGHT), colorScheme: "light" } as CSSProperties;
const TEAM_ROWS = MEMBERS.filter((member) => member.role_profile === "orchestrator" || member.role === "Worker");

function Studio({ t }: { t: number }) {
  const { portrait } = useStage();
  const painted = t >= 3.2;
  const members = TEAM_ROWS.slice(0, portrait ? 2 : 4).map((member, index) => index === 1 ? {
    ...member, title: "Pip", description: painted ? "A fresh coat. Same team." : "Picking up the paint roller",
    agent_state: { state: "working" as const, status_line: painted ? "Solarized Light applied" : "Applying Solarized Light" },
  } : member);
  const statuses = members;
  const hit = kick(t, 3.4);
  return <Set title="A fresh coat.">
    <Toy x={portrait ? 120 : 1110} y={portrait ? 177 : 76} scale={portrait ? 2.6 : 2.2} turn={-1 + kick(t, 0.55) * 3}>
      <Bubble id="cx-light-ask" text={t < 5.4 ? "A little lighter, please?" : "I meant the theme. Perfect."} width={portrait ? 320 : 330} />
    </Toy>
    <Toy x={portrait ? 120 : 70} y={(portrait ? 350 : 280) + (1 - spring(t, -0.15)) * 160 - hit * 16} scale={portrait ? 2.8 : 2.3} turn={hit * 0.8} squash={hit * 0.012}>
      <div style={{ position: "relative", width: portrait ? 300 : 760, height: portrait ? 380 : 300 }}>
        <div className="gallery-sidebar" style={{ position: "absolute", left: 0, top: 0, width: portrait ? 300 : 280 }}>
          <TeamSidebar plan={PLAN} planIndex={PLAN_INDEX} members={members} />
        </div>
        {statuses.map((member, index) => <div key={member.id} style={{ position: "absolute", left: portrait ? 6 : 320, top: portrait ? 240 + index * 70 : index * 76, width: portrait ? 284 : 430, transform: `translateY(${-kick(t, 5.05 + index * 0.12) * 5}px) scaleY(${1 - kick(t, 5.05 + index * 0.12) * 0.04})` }}>
          <div style={{ fontSize: 18, fontWeight: 650, marginBottom: 6 }}>{member.title}</div>
          <StatusRow tab={member} />
        </div>)}
      </div>
    </Toy>
    <div style={{ position: "absolute", left: portrait ? 120 : 804, top: portrait ? 300 : 1015, fontSize: portrait ? 30 : 34, color: "var(--ct-accent)", fontFamily: "var(--ct-font-mono, monospace)" }}>domios theme apply solarized-light</div>
  </Set>;
}

function FreshCoat({ t }: { t: number }) {
  const { width, height, portrait } = useStage();
  const wipe = range(t, 2.4, 4.05);
  return <>
    <Studio t={t} />
    <div style={{ ...light, position: "absolute", inset: 0, clipPath: `inset(0 ${100 * (1 - wipe)}% 0 0)` }}><Studio t={t} /></div>
    {t >= 2.2 && t < 4.65 && <Toy x={wipe * (width + 280) - 140} y={(portrait ? 700 : height / 2) - 140 + kick(t, 4.05) * 80} turn={-12 + kick(t, 4.05) * 25} squash={kick(t, 4.05) * 0.14}>
      <PaintRoller size={portrait ? 280 : 250} strokeWidth={1.5} style={{ color: "var(--ct-accent)", filter: "drop-shadow(0 14px 12px var(--ct-bg))" }} />
    </Toy>}
  </>;
}

export default { id: "cx-playful-fresh-coat", duration: 8, render: (t) => <FreshCoat t={t} /> } satisfies Scene;
