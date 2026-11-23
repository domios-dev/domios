import { useStage, type Scene } from "../../engine";
import { PLAN, PLAN_INDEX } from "../../fixtures/plan";
import { MEMBERS, TeamSidebar } from "../../fixtures/team";
import { ProductPlate, StrictStage, StrictTitle } from "./strictStage";

const staff = MEMBERS.filter((member) => member.role_profile === "orchestrator" || member.role_profile === "planner");
const workers = MEMBERS.filter((member) => !member.role_profile).slice(0, 2);

function Teams({ t }: { t: number }) {
  const { portrait } = useStage();
  const count = t < 2.4 ? 0 : t < 5 ? 1 : workers.length;
  const members = [...staff, ...workers.slice(0, count)];
  const x = portrait ? 108 : 1032;
  const y = portrait ? 648 : 216;
  return <StrictStage t={t} number="03" label="TEAMS">
    <StrictTitle lines={["One goal.", "A team."]} />
    <ProductPlate className="strict-team" x={x} y={y} width={portrait ? 864 : 768} height={portrait ? 696 : 720} scale={2.4}>
      <TeamSidebar plan={PLAN} planIndex={PLAN_INDEX} members={members} />
    </ProductPlate>
    <div style={{ position: "absolute", left: portrait ? 108 : 96, top: portrait ? 1368 : 744, fontSize: portrait ? 28 : 32, lineHeight: 1.5, color: "var(--ct-muted-fg)" }}>
      {portrait ? "Plan. Execute. Review." : <>Plan.<br />Execute.<br />Review.</>}
    </div>
  </StrictStage>;
}

const scene: Scene = { id: "cx-strict-teams", duration: 8, render: (t) => <Teams t={t} /> };
export default scene;
