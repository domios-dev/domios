import type { CSSProperties } from "react";
import { PLAN, PLAN_INDEX } from "../../fixtures/plan";
import { StatusRow } from "../../fixtures/status";
import { MEMBERS, TeamSidebar } from "../../fixtures/team";
import { SOLARIZED_LIGHT } from "../../fixtures/themes";
import { themeToCSS } from "../../../../../src/lib/themeCss";
import { roleKindLabel } from "../../../../../src/lib/roleKind";
import { Camera, range, useStage, type Scene } from "../../engine";

const REPAINT = { ...themeToCSS(SOLARIZED_LIGHT), colorScheme: "light" } as CSSProperties;
const ROWS = MEMBERS.filter((member) => member.role_profile === "orchestrator" || member.role === "Worker");
const PORTRAIT_ROWS = MEMBERS.filter((member) => member.agent_state?.state === "working");
const WIPE_FROM = 1;
const WIPE_TO = 1.45;
// The world is exactly the 1080x1920 frame at this scale, so the wipe's percentages span the whole portrait frame.
const PORTRAIT = { width: 1080 / 2.14, height: 1920 / 2.14, scale: 2.14 };

function Shot() {
  return <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)", color: "var(--ct-fg)" }}>
    <div className="gallery-sidebar" style={{ position: "absolute", left: 160, top: 150, width: 300 }}>
      <TeamSidebar plan={PLAN} planIndex={PLAN_INDEX} />
    </div>
    {ROWS.map((member, index) => (
      <div key={member.id} style={{ position: "absolute", left: 520, top: 190 + index * 120, width: 720 }}>
        <div style={{ display: "flex", alignItems: "baseline", gap: 10, padding: "0 6px 8px" }}>
          <span style={{ fontSize: 17, fontWeight: 600 }}>{member.title}</span>
          <span style={{ fontSize: 13, color: "var(--ct-muted)" }}>{roleKindLabel(member)}</span>
        </div>
        <StatusRow tab={member} />
      </div>
    ))}
  </div>;
}

function PortraitShot() {
  return <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)", color: "var(--ct-fg)" }}>
    {PORTRAIT_ROWS.map((member, index) => (
      <div key={member.id} style={{ position: "absolute", left: (PORTRAIT.width - 420) / 2, top: 120 + index * 120, width: 420 }}>
        <div style={{ display: "flex", alignItems: "baseline", gap: 10, padding: "0 6px 8px" }}>
          <span style={{ fontSize: 17, fontWeight: 600 }}>{member.title}</span>
          <span style={{ fontSize: 13, color: "var(--ct-muted)" }}>{roleKindLabel(member)}</span>
        </div>
        <StatusRow tab={member} />
      </div>
    ))}
  </div>;
}

function ThemeRepaint({ t }: { t: number }) {
  const portrait = useStage().portrait;
  const Frame = portrait ? PortraitShot : Shot;
  const wipe = range(t, WIPE_FROM, WIPE_TO) * 100;
  const keys = portrait
    ? [{ t: 0, x: PORTRAIT.width / 2, y: PORTRAIT.height / 2, scale: PORTRAIT.scale }, { t: 2.5, x: PORTRAIT.width / 2, y: PORTRAIT.height / 2, scale: PORTRAIT.scale + 0.03 }]
    : [{ t: 0, x: 700, y: 430, scale: 1.6 }, { t: 2.5, x: 720, y: 440, scale: 1.7 }];
  return (
    <Camera t={t} width={portrait ? PORTRAIT.width : 1400} height={portrait ? PORTRAIT.height : 900} keys={keys}>
      <Frame />
      {wipe > 0 && (
        <div style={{ ...REPAINT, position: "absolute", inset: 0, clipPath: `inset(0 ${100 - wipe}% 0 0)` }}>
          <Frame />
        </div>
      )}
      {wipe > 0 && wipe < 100 && <div style={{ position: "absolute", top: 0, bottom: 0, left: `${wipe}%`, width: 3, background: "var(--ct-accent)" }} />}
    </Camera>
  );
}

const themeRepaint: Scene = {
  id: "theme-repaint",
  duration: 2.5,
  render: (t) => <ThemeRepaint t={t} />,
};

export default themeRepaint;
