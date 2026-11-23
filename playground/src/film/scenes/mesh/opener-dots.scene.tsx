import { useStage, type Scene } from "../../engine";
import { BRAND_SCALE, DotField, markFrame, PosedMark, waveReleases } from "./brandField";
import { At, H, PH, PW, W } from "./meshFixtures";

const OPEN_AT = 0.15;
const DURATION = 3;
const RELEASES = waveReleases(OPEN_AT, DURATION);
const PORTRAIT_MARK = { x: PW / 2, y: 700 };

function OpenerDots({ t }: { t: number }) {
  if (useStage().portrait) {
    return (
      <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)" }}>
        <DotField t={t} releases={RELEASES} intensity={2.4} restIntensity={3.2} frameWidth={PW} frameHeight={PH} originX={PORTRAIT_MARK.x} originY={PORTRAIT_MARK.y} />
        <At x={PORTRAIT_MARK.x} y={PORTRAIT_MARK.y}>
          <PosedMark frame={markFrame(t, OPEN_AT)} size={120 * BRAND_SCALE} />
        </At>
      </div>
    );
  }
  return (
    <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)" }}>
      <DotField t={t} releases={RELEASES} intensity={2.4} restIntensity={3.2} />
      <At x={W / 2} y={H / 2}>
        <PosedMark frame={markFrame(t, OPEN_AT)} size={92 * BRAND_SCALE} />
      </At>
    </div>
  );
}

const openerDots: Scene = {
  id: "opener-dots",
  duration: DURATION,
  render: (t) => <OpenerDots t={t} />,
};

export default openerDots;
