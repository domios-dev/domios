import { useStage, type Scene } from "../../engine";
import { MachineCard } from "../mesh/meshFixtures";
import { ProductPlate, StrictStage, StrictTitle } from "./strictStage";

const destinations = ["mac-mini", "gpu-rig", "homelab"] as const;

function Mesh({ t }: { t: number }) {
  const { portrait } = useStage();
  const index = t < 2.4 ? 0 : t < 5 ? 1 : 2;
  const x = portrait ? 108 : 960;
  const y = portrait ? 792 : 384;
  return <StrictStage t={t} number="02" label="MESH">
    <StrictTitle lines={["Every", "machine.", "In reach."]} />
    <div style={{ position: "absolute", left: x, top: y - 72, fontSize: 28, color: "var(--ct-muted-fg)" }}>laptop <span style={{ color: "var(--ct-accent)", margin: "0 24px" }}>→</span> {destinations[index]}</div>
    <ProductPlate x={x} y={y} width={portrait ? 864 : 840} height={480} scale={(portrait ? 864 : 840) / 360}>
      <MachineCard key={destinations[index]} peer={destinations[index]} nameSize={18} />
    </ProductPlate>
    <div style={{ position: "absolute", left: x, top: y + 528, fontSize: 28, color: "var(--ct-accent)" }}>Ask. Send. Run.</div>
  </StrictStage>;
}

const scene: Scene = { id: "cx-strict-mesh", duration: 8, render: (t) => <Mesh t={t} /> };
export default scene;
