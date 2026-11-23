import { MemoryListTab } from "../../../../../src/components/MemoryPanel/MemoryListTab";
import { API } from "../../../gallery/fixtures";
import { range, useStage, type Scene } from "../../engine";
import { epicMemory } from "../../fixtures/codex-epic-memory";
import { MachineCard } from "../mesh/meshFixtures";
import { accent, Frame, Rule, Slam, Slug } from "./craft";

const noop = () => undefined;

function Summon({ t }: { t: number }) {
  const { portrait } = useStage();
  const landed = t >= 2.4;
  if (!landed) return <Frame>
    <Slug style={{ left: portrait ? 120 : 64, top: 100, width: portrait ? 840 : undefined, textAlign: portrait ? "center" : undefined }}>Domios / recall protocol</Slug>
    <div style={{ position: "absolute", left: portrait ? 540 : 55, top: portrait ? 330 : 245, width: portrait ? "max-content" : undefined, transform: portrait ? "translateX(-50%)" : undefined }}>
      <Slam t={t} at={0} size={portrait ? 170 : 266}>SUMMON</Slam>
      <Slam t={t} at={.45} size={portrait ? 248 : 266} style={{ marginTop: 24 }}>THE</Slam>
      <Slam t={t} at={.9} size={portrait ? 112 : 266} style={{ marginTop: portrait ? 64 : 24, color: accent }}>KNOWLEDGE.</Slam>
    </div>
    <Rule t={t} at={1.35} left={portrait ? 120 : 64} top={portrait ? 1330 : 940} width={portrait ? 840 : 1780} />
  </Frame>;
  const scale = portrait ? 1.8 : 1.72;
  const left = portrait ? 110 : 790;
  const top = (portrait ? 760 : 210) - range(t, 2.4, 7) * 12;
  const width = portrait ? 860 : 1040;
  const nodes = new Map([...epicMemory].slice(0, t < 4.7 ? 1 : t < 5.8 ? 2 : 3));
  return <Frame>
    <Slug style={{ left: portrait ? 120 : 64, top: 100 }}>Domios / recall protocol</Slug>
    <div style={{ position: "absolute", left: portrait ? 115 : 60, top: portrait ? 230 : 290 }}>
      <Slam t={t} at={2.4} size={portrait ? 130 : 116}>Or just ask</Slam>
      <Slam t={t} at={2.64} size={portrait ? 153 : 132} style={{ marginTop: 24, color: accent }}>mac-mini.</Slam>
    </div>
    <div style={{ position: "absolute", left: portrait ? 360 : 280, top: portrait ? 550 : 650, width: 360 }}><MachineCard peer="mac-mini" scale={portrait ? 1.15 : 1.35} nameSize={24} glow={.5} /></div>
    <div style={{ position: "absolute", left, top, width, height: portrait ? 600 : 650, border: "1px solid var(--ct-border)", borderRadius: 12, overflow: "hidden", background: "var(--ct-panel-bg, var(--ct-bg))" }}>
      <div style={{ padding: "20px 24px", fontSize: 30, borderBottom: "1px solid var(--ct-border)" }}>mac-mini · memory</div>
      <div style={{ width: width / scale, height: 550 / scale, transform: `scale(${scale})`, transformOrigin: "0 0" }}>
        <MemoryListTab api={API} nodes={nodes} selectedId={null} onSelectItem={noop} onViewInGraph={noop} updateNode={noop} removeNode={noop} />
      </div>
    </div>
    <Rule t={t} at={4} left={portrait ? 120 : 64} top={portrait ? 1390 : 940} width={portrait ? 840 : 1780} />
    <div style={{ position: "absolute", left: portrait ? 120 : 64, top: portrait ? 1420 : 975, width: portrait ? 840 : 1780, fontSize: portrait ? 36 : 38 }}>Ask the memory of any of your machines.</div>
  </Frame>;
}

export default { id: "cx-epic-summon", duration: 7, render: (t) => <Summon t={t} /> } satisfies Scene;
