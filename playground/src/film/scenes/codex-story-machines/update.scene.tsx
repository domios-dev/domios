import type { Scene } from "../../engine";
import { Frame, UpdateReceipt } from "./layout";
const shot: Scene = { id: "cx-story-machines-update", duration: 4, render: t =>
  <Frame title={<>The update.<br />Between turns.</>}><UpdateReceipt t={t} /></Frame> };
export default shot;
