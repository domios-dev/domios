import { useStage, type Scene } from "../../engine";
import { OWNER_ROWS } from "../../fixtures/codex-story-toldit";
import { Chat, Frame, Heading } from "./layout";

function Shot({ t }: { t: number }) {
  const { portrait } = useStage();
  return <Frame number="01"><Heading>I just told it.</Heading>
    <Chat t={t} rows={OWNER_ROWS} x={portrait ? 131.5 : 255} y={portrait ? 320 : 275} width={portrait ? 430 : 760} height={portrait ? 460 : 400} scale={portrait ? 1.9 : 1.85} />
  </Frame>;
}
const shot: Scene = { id: "cx-story-toldit-chat", duration: 5, render: (t) => <Shot t={t} /> };
export default shot;
