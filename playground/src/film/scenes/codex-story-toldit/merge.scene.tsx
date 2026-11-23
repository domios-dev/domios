import { range, useStage, type Scene } from "../../engine";
import { MERGE_ROWS } from "../../fixtures/codex-story-toldit";
import { Chat, Frame, Heading } from "./layout";

function Shot({ t }: { t: number }) {
  const { portrait } = useStage();
  const progress = range(t, 0.2, 1.8);
  return <Frame number="05"><Heading>The lane lands.</Heading>
    <Chat t={t} rows={MERGE_ROWS} x={portrait ? 131.5 : 255} y={portrait ? 325 : 285} width={portrait ? 430 : 760} height={portrait ? 330 : 220} scale={portrait ? 1.9 : 1.85} />
    <div style={{ position: "absolute", left: portrait ? 135 : 255, top: portrait ? 1060 : 780, width: portrait ? 810 : 1400 }}>
      <svg width="100%" height="130" viewBox="0 0 800 130" preserveAspectRatio="none" aria-label="Cutter lane merges into the film lane">
        <path d="M0 100H800" stroke="var(--ct-border)" strokeWidth="3" fill="none" />
        <path d="M0 10H250Q330 10 390 100H800" stroke="var(--ct-accent)" strokeWidth="4" fill="none" pathLength="1" strokeDasharray="1" strokeDashoffset={1 - progress} />
        <circle cx="390" cy="100" r={7 + progress * 3} fill="var(--ct-accent)" />
      </svg>
      <div style={{ fontSize: portrait ? 38 : 42, letterSpacing: "-0.025em" }}>lane/viris-film</div>
      <div style={{ fontSize: 27, color: "var(--ct-muted-fg)", marginTop: 15 }}>Cutter · merge bbd585fb23</div>
    </div>
  </Frame>;
}
const shot: Scene = { id: "cx-story-toldit-merge", duration: 4, render: (t) => <Shot t={t} /> };
export default shot;
