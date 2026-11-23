import "../../fixtures/supporter";
import { PLAN, TASKS } from "../../fixtures/plan";
import { StatusRow } from "../../fixtures/status";
import { MEMBERS } from "../../fixtures/team";
import { BreakoutMirror } from "../../breakout";
import { roleKindLabel } from "../../../../../src/lib/roleKind";
import type { Tab } from "../../../../../src/lib/types";
import { Camera, range, Reveal, track, useStage, type Key, type Scene } from "../../engine";
import { boardStates, TeamBoard, type BoardGeometry, type BoardMove } from "./teamBoard";

const ORCHESTRATOR = MEMBERS.find((member) => member.role_profile === "orchestrator")!;
const WORKERS = MEMBERS.filter((member) => member.role === "Worker");
const CURATOR = MEMBERS.find((member) => member.role_profile === "curator")!;
const ROWS = [ORCHESTRATOR, ...WORKERS, CURATOR];
const [MIRA, ORBIT] = WORKERS;

const WIDTH = 720;
const TOP = 250;
const STEP = 104;
const rowTop = (index: number) => TOP + index * STEP + (index > WORKERS.length ? 22 : index > 0 ? 11 : 0);

const template = TASKS.find((task) => task.status === "pending")!;
const added = (id: string, key: string, name: string) => ({ ...template, id, key, name, section_ids: ["sec-sync"] });

// Scene seconds; the reel plays this scene from 0.4 s at 0.76x, so these land on "an orchestrator plans", "workers
// build in their own lanes" and "a curator keeps the plan honest".
const MOVES: BoardMove[] = [
  { at: 3.0, task: "task-banner", to: "pending", by: ORCHESTRATOR.title, add: added("task-banner", "FN-210", "Conflict banner in the note view") },
  { at: 3.3, task: "task-retry", to: "pending", by: ORCHESTRATOR.title, add: added("task-retry", "FN-211", "Retry budget for flaky sync") },
  { at: 4.2, task: "task-background", to: "in_progress", by: MIRA.title, patch: { assigned_tab_id: MIRA.id, lane: { ...TASKS.find((task) => task.id === "task-schema")!.lane!, branch: "lane/offline-sync-background" } } },
  { at: 4.75, task: "task-qa", to: "in_review", by: ORBIT.title },
  { at: 5.8, task: "task-resolver", to: "done", by: CURATOR.title },
  { at: 6.5, task: "task-changelog", to: "pending", by: CURATOR.title },
];
const STATES = boardStates(MOVES);

/** When each member acts on the board, so its row lights up with the card it moves. */
const ACTS: Record<string, number[]> = Object.fromEntries(ROWS.map((member) => [member.id, MOVES.filter((move) => move.by === member.title).map((move) => move.at)]));

interface Layout {
  left: number;
  board: BoardGeometry;
  keys: Key[];
}

// The rows sit where the sidebar was; the board runs from beside them out past the frame's right edge.
const LANDSCAPE: Layout = {
  left: 120,
  board: { x: 900, y: 262, column: 180, gap: 12, height: 724, slot: 138 },
  keys: [
    { t: 0, x: 742, y: 600, scale: 1.45 },
    { t: 8, x: 760, y: 604, scale: 1.48 },
  ],
};

// The tall frame stacks the board under the rows, narrow enough to fit its width.
const PORTRAIT: Layout = {
  left: 650,
  board: { x: 615, y: 850, column: 150, gap: 10, height: 730, slot: 134 },
  keys: [
    { t: 0, x: 1010, y: 860, scale: 1.36 },
    { t: 8, x: 1010, y: 870, scale: 1.38 },
  ],
};

function laneOf(member: Tab): string | undefined {
  return PLAN.tasks.find((task) => task.assigned_tab_id === member.id && (task.status === "in_progress" || task.status === "in_review"))?.lane?.branch ?? undefined;
}

function Board({ t, layout }: { t: number; layout: Layout }) {
  return <TeamBoard t={t} moves={MOVES} states={STATES} geometry={layout.board} style={{ position: "absolute", inset: 0, opacity: range(t, 0.35, 1.0) }} />;
}

function TeamReturn({ t }: { t: number }) {
  const portrait = useStage().portrait;
  const layout = portrait ? PORTRAIT : LANDSCAPE;
  return (
    <>
      <Camera t={t} width={1920} height={1080} keys={layout.keys}>
        {ROWS.map((member, index) => {
          const lane = laneOf(member);
          const lit = Math.max(0, ...ACTS[member.id].map((at) => range(t, at - 0.2, at + 0.1) * (1 - range(t, at + 1.0, at + 1.5))));
          return (
            <Reveal key={member.id} t={t} at={0.15 + index * 0.1} dur={0.4} style={{ position: "absolute", left: layout.left, top: rowTop(index), width: WIDTH }}>
              <div style={{ position: "absolute", inset: "-10px -14px -6px", borderRadius: 14, border: "1.5px solid color-mix(in srgb, var(--ct-accent) 55%, transparent)", background: "color-mix(in srgb, var(--ct-accent) 7%, transparent)", opacity: lit }} />
              <div style={{ position: "relative", display: "flex", alignItems: "baseline", gap: 12, padding: "0 6px 8px", color: "var(--ct-fg)" }}>
                <span style={{ fontSize: 19, fontWeight: 650 }}>{member.title}</span>
                <span style={{ fontSize: 15, color: "var(--ct-accent)" }}>{roleKindLabel(member)}</span>
                {lane && <span style={{ marginLeft: "auto", fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 14, color: "var(--ct-accent)" }}>{lane}</span>}
              </div>
              <div style={{ position: "relative" }}>
                <StatusRow tab={member} />
              </div>
            </Reveal>
          );
        })}
        <Board t={t} layout={layout} />
      </Camera>
      {!portrait && (
        <BreakoutMirror camera={track(t, layout.keys)} world={{ width: 1920, height: 1080 }}>
          <Board t={t} layout={layout} />
        </BreakoutMirror>
      )}
    </>
  );
}

const teamReturn: Scene = {
  id: "team-return",
  duration: 8,
  render: (t) => <TeamReturn t={t} />,
};

export default teamReturn;
