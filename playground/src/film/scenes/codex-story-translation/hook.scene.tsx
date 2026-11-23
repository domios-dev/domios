import { range, useStage, type Scene } from "../../engine";
import { Frame, Label, Report, Sheet, accent, mono, muted } from "./shared";

function Hook({ t }: { t: number }) {
  const { portrait } = useStage();
  const correction = t >= 4.3;
  return <Frame step="01 / THE FLAG">
    <div style={{ position: "absolute", left: portrait ? 132 : 112, top: portrait ? 220 : 220, width: portrait ? 820 : 930 }}>
      <Label>One word. Every quest.</Label>
      <h1 style={{ margin: "30px 0 0", fontSize: portrait ? 79 : 94, fontWeight: 550, lineHeight: 1.02, letterSpacing: "-.065em" }}>One translated word broke every quest in the game.</h1>
    </div>
    <Sheet style={{ position: "absolute", left: portrait ? 132 : 1140, top: portrait ? 770 : 265, width: portrait ? 816 : 650, padding: portrait ? 38 : 42, transform: `translateY(${-range(t, 0, 6) * 9}px)` }}>
      <Label style={{ color: muted }}>Machine flag</Label>
      <div style={{ fontFamily: mono, fontSize: portrait ? 99 : 110, margin: "32px 0", display: "flex", alignItems: "center", gap: 32 }}>
        <span style={{ color: correction ? accent : muted, textDecoration: correction ? "none" : "line-through" }}>Yes</span>
        <span style={{ fontSize: 38, color: muted }}>→</span>
        <span style={{ color: "var(--ct-red)", textDecoration: correction ? "line-through" : "none" }}>Да</span>
      </div>
      <div style={{ fontSize: 28, color: correction ? accent : "var(--ct-red)" }}>{correction ? "Restore the flag. Preserve the logic." : "Quest triggers + journal: broken"}</div>
    </Sheet>
    <Report name="Reviewer" content="Machine translation changed the **Yes flag**. Quest triggers and the journal broke." width={portrait ? 544 : 620} scale={1.5} style={{ position: "absolute", left: portrait ? 132 : 112, top: portrait ? 1160 : 790 }} />
  </Frame>;
}

const hook: Scene = { id: "cx-story-translation-hook", duration: 6, render: (t) => <Hook t={t} /> };
export default hook;
