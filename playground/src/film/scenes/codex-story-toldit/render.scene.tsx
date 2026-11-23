import { StageProvider, TimeProvider, useStage, type Scene } from "../../engine";
import mesh from "../mesh/mesh-hero.scene";
import memory from "../mesh/memory-graph.scene";
import { Frame, Heading } from "./layout";

function Preview({ scene, t, x, y, width }: { scene: Scene; t: number; x: number; y: number; width: number }) {
  return <div style={{ position: "absolute", left: x, top: y, width, height: width * 1080 / 1920, border: "1px solid var(--ct-border)", overflow: "hidden" }}>
    <div style={{ width: 1920, height: 1080, transform: `scale(${width / 1920})`, transformOrigin: "top left" }}>
      <StageProvider width={1920} height={1080}><TimeProvider t={t}>{scene.render(t)}</TimeProvider></StageProvider>
    </div>
  </div>;
}
function Shot({ t }: { t: number }) {
  const { portrait } = useStage();
  return <Frame number="03"><Heading>The scenes become a film.</Heading>
    <Preview scene={mesh} t={4 + t * 0.7} x={portrait ? 135 : 120} y={portrait ? 360 : 360} width={portrait ? 810 : 810} />
    <Preview scene={memory} t={4 + t * 0.7} x={portrait ? 135 : 990} y={portrait ? 930 : 360} width={810} />
    <div style={{ position: "absolute", left: portrait ? 135 : 120, top: portrait ? 835 : 835, fontSize: 27, color: "var(--ct-muted-fg)" }}>mesh-hero · 8.0 s</div>
    <div style={{ position: "absolute", left: portrait ? 135 : 990, top: portrait ? 1405 : 835, fontSize: 27, color: "var(--ct-muted-fg)" }}>memory-graph · 8.0 s</div>
  </Frame>;
}
const shot: Scene = { id: "cx-story-toldit-render", duration: 4, render: (t) => <Shot t={t} /> };
export default shot;
