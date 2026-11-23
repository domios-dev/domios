import { JOBS_WORKSPACE } from "../../fixtures/jobs";
import { JobsView } from "../../../../../src/components/Jobs/JobsView";
import { Camera, Spotlight, useStage, type Scene } from "../../engine";

const W = 1000;
const H = 900;
// World rect of the first three cron rows, measured from the rendered list.
const CRONS = { x: 130, y: 60, w: 740, h: 208 };

function Jobs({ t }: { t: number }) {
  const portrait = useStage().portrait;
  return (
    <Camera t={t} width={W} height={H} keys={portrait ? [
      { t: 0, x: 500, y: 430, scale: 1.4 },
      { t: 2.8, x: 500, y: 440, scale: 1.44 },
      { t: 5, x: 500, y: 460, scale: 1.48 },
    ] : [
      { t: 0, x: 500, y: 250, scale: 1.88 },
      { t: 2.8, x: 500, y: 262, scale: 1.96 },
      { t: 5, x: 500, y: 300, scale: 2.02 },
    ]}>
      <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)" }}>
        <JobsView workspaces={[JOBS_WORKSPACE]} teams={[]} initialFilter={{ workspaceId: JOBS_WORKSPACE.id, teamId: null }} />
      </div>
      <Spotlight t={t} keys={[{ t: 0.3, rect: CRONS, dim: 0 },{ t: 0.8, rect: CRONS }, { t: 3.4, rect: CRONS }, { t: 4.2, rect: CRONS, dim: 0 }]} />
    </Camera>
  );
}

const jobs: Scene = {
  id: "jobs",
  duration: 5,
  render: (t) => <Jobs t={t} />,
};

export default jobs;
