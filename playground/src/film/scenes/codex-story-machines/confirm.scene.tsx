import type { Scene } from "../../engine";
import { Frame, Stack, Chat, CREW } from "./layout";
const shot: Scene = { id: "cx-story-machines-confirm", duration: 6, render: t =>
  <Frame title={<>Every agent.<br />A safe point.</>}>
    <Stack rows={3}>{CREW.map((speaker, i) => <Chat key={speaker} speaker={speaker} t={t} at={i * 0.7 - 0.1}
      text={"**Safe point confirmed.**\n\nNo build. No live writer.\nSessions preserved."} />)}</Stack>
  </Frame> };
export default shot;
