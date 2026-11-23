import { BrandMark } from "../../../../../src/components/brand/BrandMark";
import { useStage, type Scene } from "../../engine";
import { Frame } from "./layout";

function Shot() {
  const { portrait } = useStage();
  return <Frame number="06">
    <div style={{ position: "absolute", left: portrait ? 135 : 180, top: portrait ? 370 : 310, width: portrait ? 810 : 1540, textAlign: portrait ? "center" : undefined }}>
      <div style={{ display: "flex", justifyContent: portrait ? "center" : undefined, alignItems: "center", gap: 24, marginBottom: portrait ? 90 : 65 }}><BrandMark size={76} /><span style={{ fontSize: 45, letterSpacing: "-0.04em" }}>Domios</span></div>
      <div style={{ fontSize: portrait ? 102 : 120, lineHeight: 1, fontWeight: 600, letterSpacing: "-0.055em" }}>This video.</div>
      <div style={{ fontSize: portrait ? 78 : 94, marginTop: 32, lineHeight: 1.12, letterSpacing: "-0.045em", color: "var(--ct-accent)" }}>Made by just<br />talking to it.</div>
    </div>
  </Frame>;
}
const shot: Scene = { id: "cx-story-toldit-payoff", duration: 4, render: () => <Shot /> };
export default shot;
