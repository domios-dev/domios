import type { Scene } from "../../engine";
import { Frame, Stack, Chat } from "./layout";
const shot: Scene = { id: "cx-story-machines-owner", duration: 6, render: t =>
  <Frame title={<>Your machines.<br />Your call.</>}>
    <Stack rows={2}><Chat speaker="Worker" t={t} text={"**Sensor discovery**\n\nIdentify sensor interfaces before fan control.\nMQTT modes are in the plan."} />
      <Chat speaker="Orchestrator" t={t} at={0.7} text={"**Waiting for owner's OK.**\n\nLive hardware writes stay behind this gate."} /></Stack>
  </Frame> };
export default shot;
