import { PLAN, PLAN_INDEX } from "../../fixtures/plan";
import { StatusRow } from "../../fixtures/status";
import { MEMBERS, TeamSidebar } from "../../fixtures/team";
import { roleKindLabel } from "../../../../../src/lib/roleKind";
import { Camera, Reveal, useStage, type Scene } from "../../engine";

// The same order team-return shows them in: the Orchestrator, then Workers, then the Curator.
const RANK: Record<string, number> = { orchestrator: 0, curator: 2 };
const rank = (member: (typeof MEMBERS)[number]) => (member.role === "Worker" ? 1 : RANK[member.role_profile ?? ""] ?? 1);
const LANES = MEMBERS.filter((member) => member.agent_state?.state === "working").sort((a, b) => rank(a) - rank(b));

function Board({ t }: { t: number }) {
  return (
    <Camera t={t} width={1050} height={590} keys={[{ t: 0, x: 522, y: 292, scale: 1.76 }, { t: 2.5, x: 528, y: 296, scale: 1.84 }]}>
      <div className="gallery-sidebar" style={{ position: "absolute", left: 30, top: 92, width: 290 }}>
        <TeamSidebar plan={PLAN} planIndex={PLAN_INDEX} />
      </div>
      {LANES.map((member, index) => (
        <Reveal key={member.id} t={t} at={0.05 + index * 0.12} dur={0.4} style={{ position: "absolute", left: 360, top: 45 + index * 100, width: 650 }}>
          <div style={{ display: "flex", alignItems: "baseline", gap: 10, padding: "0 6px 8px" }}>
            <span style={{ fontSize: 17, fontWeight: 600 }}>{member.title}</span>
            <span style={{ fontSize: 13, color: "var(--ct-accent)" }}>{roleKindLabel(member)}</span>
          </div>
          <StatusRow tab={member} />
        </Reveal>
      ))}
    </Camera>
  );
}

function Column({ t }: { t: number }) {
  return (
    <Camera t={t} width={1080} height={1920} keys={[{ t: 0, x: 190, y: 305, scale: 3.08 }, { t: 2.5, x: 190, y: 310, scale: 3.14 }]}>
      <div className="gallery-sidebar" style={{ position: "absolute", left: 50, top: 60, width: 280 }}>
        <TeamSidebar plan={PLAN} planIndex={PLAN_INDEX} />
      </div>
    </Camera>
  );
}

function TeamGlimpse({ t }: { t: number }) {
  return useStage().portrait ? <Column t={t} /> : <Board t={t} />;
}

const teamGlimpse: Scene = {
  id: "team-glimpse",
  duration: 2.5,
  render: (t) => <TeamGlimpse t={t} />,
};

export default teamGlimpse;
