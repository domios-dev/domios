import { REVIEW_ROWS } from "../../fixtures/codex-story-toldit";
import { useStage, type Scene } from "../../engine";
import { Chat, Frame, Heading } from "./layout";

function Shot({ t }: { t: number }) {
  const { portrait } = useStage();
  const phase = t < 2.6 ? 1 : t < 4.5 ? 2 : 3;
  return <Frame number="04"><Heading>{phase === 1 ? "One draft came back." : phase === 2 ? "Cutter redid it." : "Then it passed."}</Heading>
    <Chat t={t} rows={REVIEW_ROWS} x={portrait ? 131.5 : 255} y={portrait ? 350 : 275} width={portrait ? 430 : 760} height={portrait ? 510 : 400} scale={portrait ? 1.9 : 1.85} />
  </Frame>;
}
const shot: Scene = { id: "cx-story-toldit-review", duration: 6, render: (t) => <Shot t={t} /> };
export default shot;
