import type { Scene } from "../../engine";
import { Frame, Stack, Chat, CREW } from "./layout";
const shot: Scene = { id: "cx-story-machines-restored", duration: 6, render: t =>
  <Frame title={<>Same sessions.<br />Back to work.</>}>
    <Stack rows={3}>{CREW.map((speaker, i) => <Chat key={speaker} speaker={speaker} t={t} at={i * 0.75 - 0.1}
      text={"**Back in my restored session.**\n\n" + (speaker === "General" ? "Checking every agent." : speaker === "Orchestrator" ? "The plan is still here." : "Ready to continue the task.")} />)}</Stack>
  </Frame> };
export default shot;
