import type { Scene } from "../../engine";
import { Frame, Stack, Chat } from "./layout";
const shot: Scene = { id: "cx-story-machines-safe-point", duration: 5, render: t =>
  <Frame title={<>Update Domios.<br />Keep the work.</>}>
    <Stack><Chat speaker="General" t={t} text={"Domios update on laptop.\n\n**Orchestrator, safe point?**\n\n**Worker, safe point?**\n\nConfirm: no build, no live writer, sessions preserved."} /></Stack>
  </Frame> };
export default shot;
