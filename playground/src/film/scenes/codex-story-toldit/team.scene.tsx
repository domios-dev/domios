import { DetailContent } from "../../../../../src/components/Canvas/canvasRender/DetailContent";
import { TeamSidebar } from "../../fixtures/team";
import { STORY_INDEX, STORY_MEMBERS, STORY_PLAN, STORY_TABS, STORY_TASKS } from "../../fixtures/codex-story-toldit";
import { useStage, type Scene } from "../../engine";
import { Frame, Heading } from "./layout";

function Shot({ t }: { t: number }) {
  const { portrait } = useStage();
  const lane = Math.min(2, Math.floor(Math.max(0, t - 2.5) / 1.15));
  const showLane = !portrait || t >= 2.5;
  return <Frame number="02"><Heading>{portrait && showLane ? "Its own lane." : "A whole team."}</Heading>
    {(!portrait || !showLane) && <div className="gallery-sidebar" style={{ position: "absolute", left: portrait ? 146.25 : 210, top: portrait ? 310 : 300, width: 350, maxWidth: portrait ? "none" : undefined, margin: portrait ? 0 : undefined, transform: `scale(${portrait ? 2.25 : 2})`, transformOrigin: "top left" }}>
      <TeamSidebar plan={STORY_PLAN} planIndex={STORY_INDEX} members={STORY_MEMBERS.slice(0, t < 0.7 ? 2 : t < 1.2 ? 3 : t < 1.7 ? 4 : 5)} />
    </div>}
    {showLane && <div style={{ position: "absolute", left: portrait ? 157.5 : 1040, top: portrait ? 330 : 340, width: portrait ? 425 : 430, padding: 18, background: "var(--ct-surface)", border: "1px solid var(--ct-border)", borderRadius: 10, transform: `scale(${portrait ? 1.8 : 1.7})`, transformOrigin: "top left" }}>
      <DetailContent kind="task" task={STORY_TASKS[lane]} plan={STORY_PLAN} tabs={STORY_TABS} />
    </div>}
  </Frame>;
}
const shot: Scene = { id: "cx-story-toldit-team", duration: 6, render: (t) => <Shot t={t} /> };
export default shot;
