import type { ReactNode } from "react";
import { useStage } from "../../engine";
import { OrchestratorPane, type ChatRow } from "../../fixtures/fullui-chat";
import { STORY_MEMBERS } from "../../fixtures/codex-story-toldit";

export function Frame({ children, number }: { children: ReactNode; number: string }) {
  const { portrait } = useStage();
  return <div className="toldit" style={{ position: "absolute", inset: 0, background: "var(--ct-bg)", color: "var(--ct-fg)" }}>
    <style>{`.toldit .ct-detail__mono{overflow-wrap:anywhere;white-space:normal}.toldit .ct-detail__lane dd{min-width:0}.toldit .ct-detail__prose,.toldit .ct-detail__mono,.toldit .ct-detail__agent-head,.toldit .ct-detail__role-name{font-size:14px}`}</style>
    <div style={{ position: "absolute", left: portrait ? 135 : 120, top: portrait ? 70 : 65, width: portrait ? 810 : undefined, textAlign: portrait ? "center" : undefined, fontSize: 25, color: "var(--ct-muted-fg)", letterSpacing: "0.13em" }}>DOMIOS <span style={{ marginLeft: 24, color: "var(--ct-accent)" }}>{number} / 06</span></div>
    {children}
  </div>;
}

export function Heading({ children }: { children: ReactNode }) {
  const { portrait } = useStage();
  return <div style={{ position: "absolute", left: portrait ? 135 : 120, top: portrait ? 150 : 130, width: portrait ? 810 : 1650, textAlign: portrait ? "center" : undefined, fontSize: portrait ? 65 : 76, lineHeight: 1.04, letterSpacing: "-0.045em", fontWeight: 600 }}>{children}</div>;
}

export function Chat({ t, rows, x, y, width, height, scale }: { t: number; rows: ChatRow[]; x: number; y: number; width: number; height: number; scale: number }) {
  return <div style={{ position: "absolute", left: x, top: y, width, height, transform: `scale(${scale})`, transformOrigin: "top left", border: "1px solid var(--ct-border)", borderRadius: 10, overflow: "hidden" }}>
    <OrchestratorPane t={t} tab={STORY_MEMBERS[0]} status="Working" rows={rows.filter((row) => row.at <= t)} draft="" />
  </div>;
}
