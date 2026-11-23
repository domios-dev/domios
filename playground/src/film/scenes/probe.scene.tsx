import teamSpecimen from "../../gallery/specimens/sidebar-team.spec";
import { Camera, Reveal, type Scene } from "../engine";

const team = teamSpecimen.states.find((s) => s.id === "working") ?? teamSpecimen.states[0];

const probe: Scene = {
  id: "probe",
  duration: 4,
  render: (t) => (
    <Camera t={t} width={1920} height={1080} keys={[{ t: 0, x: 960, y: 540, scale: 1 }, { t: 4, x: 400, y: 380, scale: 1.8 }]}>
      <Reveal t={t} at={0.2} style={{ position: "absolute", left: 220, top: 160, width: 360 }}>
        {team.render()}
      </Reveal>
      <div className="animate-pulse" style={{ position: "absolute", left: 1200, top: 500, width: 40, height: 40, borderRadius: 20, background: "var(--ct-accent)" }} />
    </Camera>
  ),
};

export default probe;
