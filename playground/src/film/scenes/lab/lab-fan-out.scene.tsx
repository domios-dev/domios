import "../../fixtures/supporter";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { Mail } from "lucide-react";
import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import type { Tab } from "../../../../../src/lib/types";
import { clamp, easeInOut, lerp, range, useStage, type Scene } from "../../engine";
import { PLAN, PLAN_INDEX } from "../../fixtures/plan";
import { MEMBERS, TeamSidebar } from "../../fixtures/team";
import { Ripple } from "../mesh/meshFixtures";
import { ChatRows } from "./labChat";

const DURATION = 7;
const OWNER_AT = 0.25;
const SPLIT_AT = 1.25;
const FLY = 0.9;
const REPLY_AT = 5.7;

type Job = { member: string; chip: string; working: string; done: string; leave: number; reportAt: number };
const JOBS: Job[] = [
  { member: "film-mira", chip: "release gate", working: "Running release gate v0.43…", done: "Gate green: 1,284 passed", leave: SPLIT_AT, reportAt: 3.7 },
  { member: "film-orbit", chip: "clippy · gpu-rig", working: "clippy on gpu-rig…", done: "clippy clean on gpu-rig", leave: SPLIT_AT + 0.2, reportAt: 4.1 },
  { member: "film-reel", chip: "release notes", working: "Drafting release notes…", done: "Notes drafted", leave: SPLIT_AT + 0.4, reportAt: 4.5 },
];
const REPORT_FLY = 0.7;
const ORCHESTRATOR_ID = "film-viris";

const OWNER: NormalizedChatMessage = { id: "lab-fan-owner", type: "user", content: "Ship v0.43: run the release gate, get clippy green on gpu-rig, and draft the notes." };
const REPLY: NormalizedChatMessage = { id: "lab-fan-reply", type: "assistant", content: "All three are back: **gate green**, clippy clean on gpu-rig, notes drafted. Merging now." };

const RESTING: Record<string, string> = { "film-viris": "Waiting for the owner", "film-atlas": "Plan Viris film", "film-ledger": "Curating strides into mem", "film-mira": "Idle", "film-orbit": "Idle", "film-reel": "Idle" };

function membersAt(t: number): Tab[] {
  return MEMBERS.map((tab) => {
    const job = JOBS.find((j) => j.member === tab.id);
    let description = RESTING[tab.id] ?? tab.description ?? "";
    let status = tab.agent_state;
    if (job && t >= job.leave + FLY) {
      const done = t >= job.reportAt;
      description = done ? job.done : job.working;
      status = done ? { state: "idle", status_line: job.done } : { state: "working", status_line: job.working };
    }
    if (tab.id === ORCHESTRATOR_ID) {
      const back = JOBS.filter((j) => t >= j.reportAt + REPORT_FLY).length;
      if (t >= SPLIT_AT) description = back ? `${back} of 3 reports in` : "Handing out 3 tasks";
      if (back === 3) description = "Merging 3 reports…";
    }
    return { ...tab, description, agent_state: status };
  });
}

type Point = [number, number];
const quad = (a: Point, c: Point, b: Point, k: number): Point => [
  (1 - k) ** 2 * a[0] + 2 * (1 - k) * k * c[0] + k * k * b[0],
  (1 - k) ** 2 * a[1] + 2 * (1 - k) * k * c[1] + k * k * b[1],
];

/** Row anchors measured from the real sidebar each frame; they only move when the layout does. */
function useRowAnchors(root: RefObject<HTMLDivElement | null>) {
  const [anchors, setAnchors] = useState<Record<string, Point>>({});
  useLayoutEffect(() => {
    const host = root.current;
    if (!host) return;
    const base = host.getBoundingClientRect();
    const next: Record<string, Point> = {};
    for (const tab of MEMBERS) {
      const row = host.querySelector<HTMLElement>(`.sidebar-tab[title^="${tab.title}"]`);
      if (!row) continue;
      const r = row.getBoundingClientRect();
      next[tab.id] = [r.right - base.left, r.top + r.height / 2 - base.top];
    }
    if (JSON.stringify(next) !== JSON.stringify(anchors)) setAnchors(next);
  });
  return anchors;
}

function Envelope({ at, chip, scale }: { at: Point; chip: string; scale: number }) {
  return (
    <div style={{ position: "absolute", left: at[0], top: at[1], transform: `translate(-50%, -50%) scale(${scale})`, display: "flex", alignItems: "center", gap: 8, padding: "8px 14px", borderRadius: 12, background: "var(--ct-accent)", color: "var(--ct-bg)", fontSize: 20, fontWeight: 600, whiteSpace: "nowrap", boxShadow: "0 8px 24px color-mix(in srgb, var(--ct-accent) 40%, transparent)", zIndex: 20 }}>
      <Mail size={20} />
      {chip}
    </div>
  );
}

function layout(portrait: boolean) {
  return portrait
    ? { sidebar: { x: 246, y: 140, s: 2.1 }, chat: { x: 120, y: 1130, w: 840, s: 1.55 } }
    : { sidebar: { x: 150, y: 110, s: 1.95 }, chat: { x: 960, y: 360, w: 820, s: 1.7 } };
}

function FanOut({ t }: { t: number }) {
  const { portrait } = useStage();
  const L = layout(portrait);
  const root = useRef<HTMLDivElement>(null);
  const anchors = useRowAnchors(root);
  const source: Point = [L.chat.x + L.chat.w * 0.6, L.chat.y + 40];
  const orchestrator = anchors[ORCHESTRATOR_ID];

  return (
    <div ref={root} style={{ position: "absolute", inset: 0, background: "radial-gradient(ellipse at 30% 40%, var(--ct-surface) 0%, var(--ct-bg) 70%)" }}>
      <div className="gallery-sidebar" style={{ position: "absolute", left: L.sidebar.x, top: L.sidebar.y, width: 300, transform: `scale(${L.sidebar.s})`, transformOrigin: "0 0" }}>
        <TeamSidebar plan={PLAN} planIndex={PLAN_INDEX} members={membersAt(t)} />
      </div>
      <div style={{ position: "absolute", left: L.chat.x, top: L.chat.y, width: L.chat.w / L.chat.s, transform: `scale(${L.chat.s})`, transformOrigin: "0 0" }}>
        <ChatRows t={t} rows={[{ message: OWNER, at: OWNER_AT }, { message: REPLY, at: REPLY_AT }]} padding="0" />
      </div>
      {JOBS.map((job, i) => {
        const target = anchors[job.member];
        if (!target) return null;
        const k = range(t, job.leave, job.leave + FLY, easeInOut);
        if (k <= 0 || k >= 1) return null;
        const control: Point = [lerp(source[0], target[0], 0.5), Math.min(source[1], target[1]) - (portrait ? 160 : 120) - i * 30];
        return <Envelope key={job.member} at={quad(source, control, [target[0] + 40, target[1]], k)} chip={job.chip} scale={lerp(1.15, 0.7, k)} />;
      })}
      <svg style={{ position: "absolute", inset: 0, width: "100%", height: "100%", overflow: "visible", zIndex: 25, pointerEvents: "none" }} aria-hidden="true">
        <Ripple t={t} x={source[0]} y={source[1]} at={SPLIT_AT - 0.1} size={120} />
        {JOBS.map((job) => {
          const from = anchors[job.member];
          if (!from || !orchestrator) return null;
          const k = range(t, job.reportAt, job.reportAt + REPORT_FLY, easeInOut);
          const side = portrait ? 150 : 120;
          const control: Point = [from[0] + side, (from[1] + orchestrator[1]) / 2];
          const [x, y] = quad(from, control, orchestrator, k);
          const trail = quad(from, control, orchestrator, clamp(k - 0.15));
          return (
            <g key={job.member}>
              <Ripple t={t} x={from[0] + 20} y={from[1]} at={job.leave + FLY} size={46} />
              {k > 0 && k < 1 && (
                <>
                  <line x1={trail[0]} y1={trail[1]} x2={x} y2={y} stroke="var(--ct-green)" strokeWidth={6} strokeLinecap="round" opacity={0.5} />
                  <circle cx={x} cy={y} r={20} fill="var(--ct-green)" opacity={0.2} />
                  <circle cx={x} cy={y} r={8} fill="var(--ct-green)" />
                </>
              )}
              <Ripple t={t} x={orchestrator[0]} y={orchestrator[1]} at={job.reportAt + REPORT_FLY} size={40} />
            </g>
          );
        })}
      </svg>
    </div>
  );
}

const fanOut: Scene = {
  id: "lab-fan-out",
  duration: DURATION,
  render: (t) => <FanOut t={t} />,
};

export default fanOut;
