import { range, useStage, type Scene } from "../../engine";
import { Frame, Label, Report, Sheet, accent, mono, muted } from "./shared";

function Release({ t }: { t: number }) {
  const { portrait } = useStage();
  return <Frame step="05 / SHIPPED">
    <div style={{ position: "absolute", left: portrait ? 132 : 112, top: portrait ? 205 : 185 }}>
      <Label>Dread Delusion / Russian localization</Label>
      <div style={{ fontSize: portrait ? 186 : 242, letterSpacing: "-.085em", lineHeight: 1.12, fontWeight: 550, color: accent, transform: `translateX(${range(t, 0, 6) * 6}px)` }}>1.3.3</div>
      <h1 style={{ fontSize: portrait ? 74 : 85, letterSpacing: "-.055em", fontWeight: 550, lineHeight: 1.05, margin: "26px 0" }}>Ported. Reviewed.<br />Released.</h1>
      <div style={{ fontSize: 30, color: muted }}>Steam Deck instructions included.</div>
    </div>
    <Sheet style={{ position: "absolute", left: portrait ? 132 : 1140, top: portrait ? 835 : 205, width: portrait ? 816 : 650, padding: 36 }}>
      <Label>Release record</Label>
      <div style={{ fontFamily: mono, fontSize: 29, marginTop: 30, lineHeight: 1.8 }}>
        <div>43 TextAssets</div>
        <div>≈13,400 non-empty RU lines</div>
        <div style={{ color: muted }}>across 43 files</div>
        <div style={{ marginTop: 14, paddingTop: 14, borderTop: "1px solid var(--ct-border)", color: muted }}>20 files · second literary pass</div>
      </div>
    </Sheet>
    <Report name="Release" content="**Release 1.3.3** · Steam Deck instructions included.\n\nPort, editing & packaging: rollacode." width={portrait ? 544 : 770} scale={1.5} style={{ position: "absolute", left: portrait ? 132 : 112, top: portrait ? 1200 : 790 }} />
  </Frame>;
}

const release: Scene = { id: "cx-story-translation-release", duration: 6, render: (t) => <Release t={t} /> };
export default release;
