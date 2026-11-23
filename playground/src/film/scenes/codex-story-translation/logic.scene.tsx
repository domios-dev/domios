import { useStage, type Scene } from "../../engine";
import { Frame, Label, Report, Sheet, accent, mono, muted } from "./shared";

function Logic({ t }: { t: number }) {
  const { portrait } = useStage();
  return <Frame step="03 / THE RULE">
    <div style={{ position: "absolute", left: portrait ? 132 : 112, top: portrait ? 210 : 190, width: portrait ? 816 : 1000 }}>
      <Label>The rule after Yes</Label>
      <h1 style={{ margin: "24px 0", fontSize: portrait ? 79 : 98, lineHeight: 1.04, fontWeight: 550, letterSpacing: "-.06em" }}>IDs and flags stay,<br /><span style={{ color: accent }}>prose gets translated.</span></h1>
    </div>
    <Sheet style={{ position: "absolute", left: portrait ? 132 : 1150, top: portrait ? 650 : 208, width: portrait ? 816 : 650, padding: 38, fontFamily: mono }}>
      <Label style={{ color: muted }}>Invented example / review card</Label>
      <div style={{ fontSize: 30, marginTop: 32, display: "grid", gap: 23 }}>
        <div><span style={{ color: muted }}>id</span> · demo_001 <span style={{ color: accent }}>✓ kept</span></div>
        <div><span style={{ color: muted }}>flag</span> · Yes <span style={{ color: accent }}>✓ kept</span></div>
        <div style={{ borderTop: "1px solid var(--ct-border)", paddingTop: 26, color: muted }}>prose</div>
        <div style={{ color: t < 2 ? muted : accent }}>{t < 2 ? "The quiet lantern." : "Тихий фонарь."}</div>
        <div style={{ fontSize: 24, color: muted }}>Markup + placeholders preserved</div>
      </div>
    </Sheet>
    <Report name="RUS-Items" content="**Review checklist**\n\nIDs, flags, markup, placeholders: preserved. Glossary checked against English." width={portrait ? 544 : 620} scale={1.5} style={{ position: "absolute", left: portrait ? 132 : 112, top: portrait ? 1140 : 732 }} />
    {!portrait && <div style={{ position: "absolute", left: 1150, top: 800, fontFamily: mono, fontSize: 26, color: muted }}>43 TextAssets · EN / RU pairs</div>}
  </Frame>;
}

const logic: Scene = { id: "cx-story-translation-logic", duration: 6, render: (t) => <Logic t={t} /> };
export default logic;
