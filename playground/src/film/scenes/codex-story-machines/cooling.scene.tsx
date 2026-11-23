import type { Scene } from "../../engine";
import { Frame, Stack, Chat, CoolingTask } from "./layout";
const shot: Scene = { id: "cx-story-machines-cooling", duration: 5, render: t =>
  <Frame title={<>A cooler PC.<br />A careful plan.</>}>
    <Stack rows={2}><Chat speaker="Orchestrator" t={t} text={"**Fan control on real hardware + MQTT modes.**\n\nOne executor. Discovery first."} />
      <CoolingTask /></Stack>
  </Frame> };
export default shot;
