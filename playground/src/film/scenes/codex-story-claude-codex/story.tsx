import type { CSSProperties, ReactNode } from "react";
import { Check, GitMerge } from "lucide-react";
import { AgentIcon } from "../../../../../src/components/AgentIcon";
import { BoardTaskCard } from "../../../../../src/components/Canvas/planView/BoardTaskCard";
import { InboundReportCard } from "../../../../../packages/chat-ui/src/codeterm/InboundReportCard";
import { ChatNoticeCard } from "../../../../../packages/chat-ui/src/ChatNoticeCard";
import { TeamSidebar } from "../../fixtures/team";
import { CUTTER_COMMIT, CUTTER_MERGE, LANE_TASKS, REPORTS, STORY_INDEX, STORY_MEMBERS, STORY_PLAN, taskCard } from "../../fixtures/codex-story-claude-codex";
import { range, useStage } from "../../engine";

const noop = () => undefined;
const panel: CSSProperties = { background: "var(--ct-surface)", border: "1px solid var(--ct-border)", borderRadius: 16, boxShadow: "0 22px 80px color-mix(in srgb, var(--ct-bg) 60%, transparent)" };

function Frame({ children, number }: { children: ReactNode; number: string }) {
  const { portrait } = useStage();
  return <div className="cx-claude-story" style={{ position: "absolute", inset: 0, background: "var(--ct-bg)", color: "var(--ct-fg)", fontFamily: "var(--ct-font-ui)" }}>
    <style>{`.cx-claude-story .ct-card__body--reveal { animation: none; } .cx-claude-story .gallery-sidebar { background: var(--ct-surface); }`}</style>
    <div style={{ position: "absolute", inset: 0, background: "radial-gradient(ellipse at 25% 15%, color-mix(in srgb, var(--ct-accent) 8%, transparent), transparent 65%)" }} />
    <div style={{ position: "absolute", left: portrait ? 130 : 100, top: portrait ? 80 : 65, width: portrait ? 820 : undefined, textAlign: portrait ? "center" : undefined, fontSize: 24, letterSpacing: 4, color: "var(--ct-muted-fg)" }}>DOMIOS <span style={{ marginLeft: 30, color: "var(--ct-accent)" }}>{number} / 05</span></div>
    {children}
  </div>;
}

function Zoom({ x, y, width, scale = 2, children, style }: { x: number; y: number; width: number; scale?: number; children: ReactNode; style?: CSSProperties }) {
  return <div style={{ position: "absolute", left: x, top: y, width, transform: `scale(${scale})`, transformOrigin: "top left", ...style }}>{children}</div>;
}

function Team({ x, y, scale }: { x: number; y: number; scale: number }) {
  const { portrait } = useStage();
  return <Zoom x={x} y={y} width={360} scale={scale}><div className="gallery-sidebar" style={{ ...panel, padding: 12, maxWidth: portrait ? "none" : undefined, margin: portrait ? 0 : undefined }}><TeamSidebar plan={STORY_PLAN} planIndex={STORY_INDEX} members={STORY_MEMBERS} /></div></Zoom>;
}

function Title({ children, x, y, width, size = 74 }: { children: ReactNode; x: number; y: number; width: number; size?: number }) {
  const { portrait } = useStage();
  return <div style={{ position: "absolute", left: x, top: y, width, textAlign: portrait ? "center" : undefined, fontSize: size, fontWeight: 650, lineHeight: 1.08, letterSpacing: "-0.055em" }}>{children}</div>;
}

export function Hook({ t }: { t: number }) {
  const { portrait } = useStage();
  return <Frame number="01">
    <Title x={portrait ? 130 : 105} y={portrait ? 175 : 240} width={portrait ? 820 : 830} size={portrait ? 116 : 128}>Claude<br />runs <span style={{ color: "var(--ct-accent)" }}>Codex.</span></Title>
    <div style={{ position: "absolute", left: portrait ? 130 : 112, width: portrait ? 820 : undefined, textAlign: portrait ? "center" : undefined, top: portrait ? 480 : 590, color: "var(--ct-muted-fg)", fontSize: portrait ? 30 : 32 }}>This is the team making this film.</div>
    <Team x={portrait ? 207 : 1080 - range(t, 0, 4) * 35} y={portrait ? 600 : 170} scale={portrait ? 1.85 : 1.95} />
  </Frame>;
}

export function Lanes({ t }: { t: number }) {
  const { portrait } = useStage();
  const crew = [STORY_MEMBERS[1], STORY_MEMBERS[2], STORY_MEMBERS[5]];
  const scale = portrait ? 2 : 1.9;
  return <Frame number="02">
    <Title x={portrait ? 130 : 80} y={portrait ? 175 : 155} width={portrait ? 820 : 1600} size={portrait ? 72 : 88}>One lead.<br />Separate lanes.</Title>
    {crew.map((member, i) => {
      const x = portrait ? 130 : 80 + i * 608;
      const y = portrait ? 390 + i * 350 : 430 + (1 - range(t, 0, 0.7)) * (i + 1) * 10;
      return <Zoom key={member.id} x={x} y={y} width={portrait ? 410 : 294} scale={scale}>
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "0 6px 12px", fontSize: 17 }}><AgentIcon providerId={member.agent_type} style={{ fontSize: 24 }} /><div><strong>{member.title}</strong><div style={{ fontSize: 14, color: "var(--ct-muted-fg)" }}>{i === 0 ? "Claude Opus" : i === 1 ? "Codex GPT-6.1-Sol" : "Claude Sonnet"}</div></div></div>
        <BoardTaskCard card={taskCard(LANE_TASKS[i])} />
      </Zoom>;
    })}
  </Frame>;
}

export function Reports({ t }: { t: number }) {
  const { portrait } = useStage();
  return <Frame number="03">
    <Title x={portrait ? 130 : 85} y={portrait ? 175 : 175} width={portrait ? 820 : 650} size={portrait ? 82 : 102}>The work<br />comes back.</Title>
    {REPORTS.map((report, i) => <Zoom key={report.sender} x={portrait ? 125 : 840} y={portrait ? 430 + i * 300 : 160 + i * 265} width={portrait ? 415 : 490} scale={portrait ? 2 : 1.95}>
      <InboundReportCard message={report.message} sender={report.sender} expanded={i === 2 && t >= 1} onToggle={noop} fontScale={2} />
    </Zoom>)}
  </Frame>;
}

export function Review({ t }: { t: number }) {
  const { portrait } = useStage();
  const accepted = t >= 2;
  return <Frame number="04">
    <Title x={portrait ? 130 : 85} y={portrait ? 175 : 180} width={portrait ? 820 : 770} size={portrait ? 84 : 104}>A report.<br />A real review.</Title>
    <Zoom x={portrait ? 130 : 950} y={portrait ? 430 : 190} width={portrait ? 410 : 430} scale={2}>
      <BoardTaskCard card={taskCard({ ...LANE_TASKS[2], status: accepted ? "done" : "in_review" })} />
    </Zoom>
    <Zoom x={portrait ? 130 : 85} y={portrait ? 850 : 600} width={portrait ? 410 : 820} scale={portrait ? 2 : 1.9}>
      <ChatNoticeCard testId="cx-cutter-review" icon={<Check size={18} />} title={accepted ? "Viris · Accepted" : "Viris · Reviewing"} subtitle="Cutter · Reels, batch capture and review sheets" tone={accepted ? "var(--ct-green)" : "var(--ct-accent)"} expanded onToggle={noop}>
        <div style={{ padding: 8, fontSize: 17, lineHeight: 1.5 }}>{accepted ? "Reviewed the serious contact sheet. All cuts are hard, and luma passes on all 22 clips." : "Checking the contact sheet, cut order and luma results."}</div>
      </ChatNoticeCard>
    </Zoom>
  </Frame>;
}

function MergeGraph({ t, portrait }: { t: number; portrait: boolean }) {
  const progress = range(t, 0, 1.1);
  return <div style={{ position: "absolute", left: portrait ? 132.5 : 110, top: portrait ? 480 : 485, width: portrait ? 815 : 1680 }}>
    <svg viewBox="0 0 900 160" width="100%" style={{ overflow: "visible" }} aria-label="Reviewed Cutter lane joins the film branch in a real merge">
      <path d="M20 120 H880" fill="none" stroke="var(--ct-border)" strokeWidth="3" />
      <path d="M120 120 C180 120 160 35 240 35 H580 C650 35 650 120 730 120" fill="none" stroke="var(--ct-magenta)" strokeWidth="4" pathLength={1} strokeDasharray={1} strokeDashoffset={1 - progress} />
      <circle cx="120" cy="120" r="9" fill="var(--ct-fg)" /><circle cx="580" cy="35" r="10" fill="var(--ct-magenta)" /><circle cx="730" cy="120" r="13" fill="var(--ct-green)" />
      <text x="250" y="20" fill="var(--ct-muted-fg)" fontSize={portrait ? 30 : 20} fontFamily="var(--ct-font-mono)">{CUTTER_COMMIT} · reviewed</text>
      <text x="600" y="158" fill="var(--ct-green)" fontSize={portrait ? 30 : 20} fontFamily="var(--ct-font-mono)">{CUTTER_MERGE}</text>
    </svg>
  </div>;
}

export function Payoff({ t }: { t: number }) {
  const { portrait } = useStage();
  return <Frame number="05">
    <Title x={portrait ? 130 : 85} y={portrait ? 175 : 180} width={portrait ? 820 : 1700} size={portrait ? 112 : 142}>Any agent.<br /><span style={{ color: "var(--ct-accent)" }}>One team.</span></Title>
    <MergeGraph t={t} portrait={portrait} />
    <Zoom x={portrait ? 130 : 110} y={portrait ? 760 : 860} width={portrait ? 410 : 820} scale={portrait ? 2 : 1.9}>
      <ChatNoticeCard testId="cx-cutter-merge" icon={<GitMerge size={18} />} title="Merged · lane/viris-film" subtitle={`${CUTTER_MERGE} · two parents · review accepted by Viris`} tone="var(--ct-green)" expanded={false} onToggle={noop}><span /></ChatNoticeCard>
    </Zoom>
  </Frame>;
}
