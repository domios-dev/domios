import type { ReactNode } from "react";
import { Camera, sequence, useStage, type Scene } from "../../engine";
import { ChatRows } from "../lab/labChat";
import { InboundReportCard } from "../../../../../packages/chat-ui/src/codeterm/InboundReportCard";
import { TeamSidebar } from "../../fixtures/team";
import { DetailContent } from "../../../../../src/components/Canvas/canvasRender/DetailContent";
import { OrchestratorPane } from "../../fixtures/fullui-chat";
import { StatusRow } from "../../fixtures/status";
import { FLOW_INDEX, FLOW_MEMBERS, FLOW_PLAN, FLOW_TABS, FLOW_TASK, MERGE, REPORT, REQUEST } from "../../fixtures/mob-flow";

const DURATION = 6;
const noop = () => undefined;

function Take({ t, travel = 130, centre = 502.5, children }: { t: number; travel?: number; centre?: number; children: ReactNode }) {
  const stage = useStage();
  const fit = Math.min(stage.width / 1080, stage.height / 1920);
  return <div className="mob-flow" style={{ position: "absolute", inset: 0, background: "var(--ct-bg)", color: "var(--ct-fg)" }}>
    <style>{stage.portrait ? `.mob-flow .gallery-sidebar{max-width:none;margin:0}` : ""}</style>
    <style>{`
      .mob-flow .ct-detail__mono{overflow-wrap:anywhere;white-space:normal}
      .mob-flow .ct-detail__lane dd{min-width:0}
      .mob-flow .ct-detail__prose,.mob-flow .ct-detail__mono,.mob-flow .ct-detail__agent-sub,.mob-flow .ct-detail__role-name,.mob-flow .ct-detail__fact,.mob-flow .ct-detail__status,.mob-flow .ct-detail__heading,.mob-flow .ct-detail__lane dt{font-size:14px;letter-spacing:normal;text-transform:none}
      .mob-flow .ct-detail__persona{font-size:18px}
      .mob-flow [data-testid="tab-description-subtitle"],.mob-flow .sidebar-tab-body__sub{font-size:12px;line-height:14px}
      .mob-flow-review .ct-detail > :not(.ct-detail__meta):not(.ct-detail__agent){display:none}
    `}</style>
    <div style={{ position: "absolute", width: 1080, height: 1920, left: (stage.width - 1080 * fit) / 2, top: (stage.height - 1920 * fit) / 2, transform: `scale(${fit})`, transformOrigin: "top left" }}>
      <div style={{ position: "absolute", inset: "0 0 500px", overflow: "hidden", maskImage: "linear-gradient(to bottom, var(--ct-bg) 94%, transparent)" }}>
        <Camera t={t} width={1080} height={2500} keys={[
          { t: 0, x: stage.portrait ? centre : 540, y: 710, scale: 1 },
          { t: DURATION, x: stage.portrait ? centre : 540, y: 710 + travel, scale: 1.035 },
        ]}>
          {children}
        </Camera>
      </div>
    </div>
  </div>;
}

function ReviewCanvas({ t, accepted = false }: { t: number; accepted?: boolean }) {
  const task = { ...FLOW_TASK, status: accepted && t >= 1.6 ? "done" as const : FLOW_TASK.status };
  const row = accepted ? MERGE[0] : REPORT[1];
  return <div style={{ position: "absolute", left: 60, top: 180, width: 354, transform: "scale(2.5)", transformOrigin: "top left" }}>
    <div className="mob-flow-review">
      <DetailContent kind="task" task={task} plan={FLOW_PLAN} tabs={FLOW_TABS} />
    </div>
    <div style={{ marginTop: 16 }}>
      <InboundReportCard message={row.message} sender="Mira" expanded onToggle={noop} />
    </div>
    {accepted && <div style={{ marginTop: 30 }}>
      <ChatRows t={t} rows={[MERGE[1]]} padding="0" />
    </div>}
    <div style={{ marginTop: 30 }}><StatusRow tab={FLOW_MEMBERS[0]} /></div>
  </div>;
}

const request: Scene = {
  id: "mob-flow-request", duration: DURATION,
  render: (t) => <Take t={t} travel={40} centre={498.6}>
    <div style={{ position: "absolute", left: 60, top: 170, width: 258, height: 355, transform: "scale(3.4)", transformOrigin: "top left", border: "1px solid var(--ct-border)", borderRadius: 12, overflow: "hidden", background: "var(--ct-bg)" }}>
      <OrchestratorPane t={t} tab={FLOW_MEMBERS[0]} status="Working" rows={REQUEST.map((row) => ({ ...row, at: row.at > 0 ? 0.05 : row.at }))} draft="" />
    </div>
  </Take>,
};

const team: Scene = {
  id: "mob-flow-team", duration: DURATION,
  render: (t) => <Take t={t} travel={170} centre={509}>
    <div className="gallery-sidebar" style={{ position: "absolute", left: 104, top: 350, width: 300, transform: "scale(2.7)", transformOrigin: "top left" }}>
      <TeamSidebar plan={FLOW_PLAN} planIndex={FLOW_INDEX} members={FLOW_MEMBERS.map((member, index) => index > 1 && t < (index - 1) * 1.2 ? { ...member, agent_state: { state: "idle" as const, status_line: "Ready" } } : member)} />
    </div>
  </Take>,
};

const lane: Scene = {
  id: "mob-flow-lane", duration: DURATION,
  render: (t) => <Take t={t} travel={210} centre={507.5}>
    <div style={{ position: "absolute", left: 100, top: 300, width: 326, transform: "scale(2.5)", transformOrigin: "top left" }}>
      <DetailContent kind="task" task={FLOW_TASK} plan={FLOW_PLAN} tabs={FLOW_TABS} />
    </div>
  </Take>,
};

const report: Scene = {
  id: "mob-flow-report", duration: DURATION,
  render: (t) => <Take t={t} travel={100}><ReviewCanvas t={t} /></Take>,
};

const merge: Scene = {
  id: "mob-flow-merge", duration: DURATION,
  render: (t) => <Take t={t} travel={180}><ReviewCanvas t={t} accepted /></Take>,
};

export default [request, team, lane, report, merge, sequence("mob-flow-reel", [request, team, lane, report, merge])];
