import { useStage, type Scene } from "../../engine";
import { Frame, Label, Report, Sheet, accent, muted } from "./shared";

function Fonts() {
  const { portrait } = useStage();
  return <Frame step="04 / TWO TEXT SYSTEMS">
    <div style={{ position: "absolute", left: portrait ? 132 : 112, top: portrait ? 205 : 165 }}>
      <Label>Cyrillic, without the boxes</Label>
      <h1 style={{ margin: "24px 0", fontSize: portrait ? 82 : 92, letterSpacing: "-.06em", lineHeight: 1.04, fontWeight: 550 }}>Two font fixes.<br />Still alagard.</h1>
    </div>
    {[
      { title: "Legacy UI", fix: "Embedded Cyrillic font", note: "Font embedded for the legacy UI" },
      { title: "TextMeshPro", fix: "BepInEx plugin", note: "Cyrillic fallback for TextMeshPro" },
    ].map((system, i) => <Sheet key={system.title} style={{ position: "absolute", left: portrait ? 132 : 112 + i * 864, top: portrait ? 540 + i * 340 : 430, width: portrait ? 816 : 820, height: 310, padding: 36 }}>
      <Label>{system.title}</Label>
      <div style={{ marginTop: 23, fontSize: portrait ? 45 : 47, letterSpacing: "-.04em", color: accent }}>{system.fix}</div>
      <div style={{ marginTop: 16, fontSize: 27, color: muted }}>{system.note}</div>
      <div style={{ marginTop: 25, fontSize: 30 }}>alagard <span style={{ color: muted }}>+ кириллица</span></div>
    </Sheet>)}
    <Report name="Font review" content="**Original alagard preserved.**\n\nCyrillic resolved in both systems. No missing-glyph boxes." width={portrait ? 544 : 770} scale={1.5} style={{ position: "absolute", left: portrait ? 132 : 112, top: portrait ? 1200 : 760 }} />
  </Frame>;
}

const fonts: Scene = { id: "cx-story-translation-fonts", duration: 7, render: () => <Fonts /> };
export default fonts;
