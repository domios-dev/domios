import { BrandMark } from "../../../../../src/components/brand/BrandMark";
import { useBreakout } from "../../breakout";
import { easeOut, lerp, range, useStage, type Scene } from "../../engine";
import { BRAND_SCALE, DotField } from "./brandField";
import { At, H, PH, PW, W } from "./meshFixtures";

const RELEASES = [0.2];
// On a page the waves have room past the frame; a second one carries the spread further out.
const PAGE_RELEASES = [0.2, 1.3];

const LANDSCAPE = { x: W / 2, markY: H / 2 - 70, wordY: H / 2 + 150, mark: 92, word: 76, line: 26, gap: 14 };
const PORTRAIT = { x: PW / 2, markY: 620, wordY: 930, mark: 120, word: 112, line: 38, gap: 20 };

function EndCard({ t }: { t: number }) {
  const portrait = useStage().portrait;
  const at = portrait ? PORTRAIT : LANDSCAPE;
  const releases = useBreakout() ? PAGE_RELEASES : RELEASES;
  const mark = range(t, 0, 0.7, easeOut);
  const word = range(t, 0.45, 1.1, easeOut);
  const line = range(t, 0.8, 1.5, easeOut);
  return (
    <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)" }}>
      {portrait
        ? <DotField t={t} releases={releases} intensity={1.6} restIntensity={3.2} frameWidth={PW} frameHeight={PH} originX={at.x} originY={at.markY} />
        : <DotField t={t} releases={releases} intensity={1.6} restIntensity={3.2} />}
      <At x={at.x} y={at.markY}>
        <div data-film-anchor="mark" style={{ opacity: mark, transform: `scale(${lerp(0.9, 1, mark)})` }}>
          <BrandMark size={at.mark * BRAND_SCALE} />
        </div>
      </At>
      <At x={at.x} y={at.wordY}>
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: at.gap }}>
          <div style={{ opacity: word, transform: `translateY(${(1 - word) * 14}px)`, fontSize: at.word, fontWeight: 700, letterSpacing: "-0.04em", color: "var(--ct-fg)", lineHeight: 1 }}>domios</div>
          <div style={{ opacity: line, transform: `translateY(${(1 - line) * 10}px)`, fontSize: at.line, color: "var(--ct-muted-fg)", letterSpacing: "0.01em", whiteSpace: "nowrap" }}>Agents live here.</div>
        </div>
      </At>
    </div>
  );
}

const endCard: Scene = {
  id: "end-card",
  duration: 3,
  render: (t) => <EndCard t={t} />,
};

export default endCard;
