import { TabItem } from "../../../../../src/components/Sidebar/TabItem";
import type { Tab } from "../../../../../src/lib/types";
import { ProductProviders } from "../../../gallery/fixtures";
import { range, useStage, type Scene } from "../../engine";
import { Frame, Label, Report, Sheet, accent, mono, muted } from "./shared";

const names = ["RUS-Endless", "RUS-HallowQuest", "RUS-Isles", "RUS-Items"];
const tabs: Tab[] = names.map((title, i) => ({ id: `translation-${i}`, title, pty_id: `translation-pty-${i}`, tab_type: "Terminal", role: "Worker", role_profile: null, agent_type: null, agent_session_id: `translation-session-${i}`, workspace_id: "translation", parent_tab_id: null, team_id: null, cwd: "/dread-delusion", description: "Editor → separate reviewer", agent_state: { state: "working", status_line: "Checking RU against EN" } } as Tab));
const noop = () => undefined;

function Lanes({ t }: { t: number }) {
  const { portrait } = useStage();
  return <Frame step="02 / PARALLEL LANES">
    <div style={{ position: "absolute", left: portrait ? 132 : 112, top: portrait ? 212 : 170, width: portrait ? 816 : 1650 }}>
      <Label>Port. Edit. Review.</Label>
      <h1 style={{ fontSize: portrait ? 72 : 86, lineHeight: 1.05, letterSpacing: "-.055em", fontWeight: 550, margin: "20px 0" }}>An existing translation.<br />A team to carry it forward.</h1>
      <div style={{ fontSize: 27, color: muted }}>LiquidRing’s base + machine-translated new content</div>
    </div>
    <ProductProviders tabs={tabs} workspaceId="translation">
      {tabs.map((tab, i) => <Sheet key={tab.id} style={{ position: "absolute", left: portrait ? 132 : 112 + i * 430, top: portrait ? 520 + i * 126 : 490, width: portrait ? 816 : 406, height: portrait ? 122 : 166, overflow: "hidden", borderTop: `2px solid ${accent}` }}>
        <div style={{ width: portrait ? 480 : 270, transform: `scale(${portrait ? 1.7 : 1.5})`, transformOrigin: "top left", paddingTop: 8 }}>
          <TabItem tab={tab} index={i} isActive={i === Math.min(3, Math.floor(t / 2))} workspaceId="translation" hasAgent status="Working" canClose={false} onClick={noop} onClose={noop} uiFontScale={2} />
        </div>
      </Sheet>)}
    </ProductProviders>
    <div style={{ position: "absolute", left: portrait ? 132 : 112, top: portrait ? 1060 : 738, width: portrait ? 816 : 790 }}>
      <Label>Largest dialogue file → 4 worktrees</Label>
      <div style={{ display: "flex", gap: 14, marginTop: 24 }}>
        {[0, 1, 2, 3].map((i) => <div key={i} style={{ flex: 1, borderTop: `3px solid ${accent}`, padding: "17px 0", fontFamily: mono, fontSize: 26, color: muted }}>
          <span style={{ color: accent }}>lane {i + 1}</span>
          <div style={{ height: 5, background: "var(--ct-border)", marginTop: 17 }}><div style={{ height: "100%", background: accent, width: `${35 + 65 * range(t, i * .3, 7)}%` }} /></div>
        </div>)}
      </div>
    </div>
    <Report name="Reviewer" content="**Editor → separate reviewer**\n\nEnglish comparison. Shared glossary. Lore notes." width={portrait ? 544 : 540} scale={1.5} style={{ position: "absolute", left: portrait ? 132 : 980, top: portrait ? 1205 : 755 }} />
  </Frame>;
}

const lanes: Scene = { id: "cx-story-translation-lanes", duration: 8, render: (t) => <Lanes t={t} /> };
export default lanes;
