import { PLAN, PLAN_ID, PLAN_TABS, PRODUCT_TASK, TASKS } from "../../fixtures/plan";
import { DetailContent } from "../../../../../src/components/Canvas/canvasRender/DetailContent";
import { PlanStatusBreakdown } from "../../../../../src/components/shared/PlanStatusBreakdown";
import { SegmentedDonut } from "../../../../../src/components/shared/SegmentedDonut";
import { Camera, Spotlight, useStage, type Scene } from "../../engine";

const count = (...statuses: string[]) => TASKS.filter((task) => statuses.includes(task.status)).length;
const COUNTS = { todo: count("pending", "in_progress"), review: count("in_review"), blocked: count("blocked"), paused: 0, done: count("done") };
const SEGMENTS = [
  { status: "todo" as const, count: COUNTS.todo },
  { status: "review" as const, count: COUNTS.review },
  { status: "blocked" as const, count: COUNTS.blocked },
  { status: "done" as const, count: COUNTS.done },
];

const open = () => undefined;
const LEFT = 400;
const WIDTH = 1120;
// World rects of the owner card and the review stride, measured from the rendered detail.
const OWNER = { x: 412, y: 366, w: 1096, h: 183 };
const PROOF = { x: 430, y: 1003, w: 1068, h: 47 };

function PlanBoard({ t }: { t: number }) {
  const portrait = useStage().portrait;
  return (
    <Camera t={t} width={1920} height={1080} keys={portrait ? [
      { t: 0, x: 800, y: 520, scale: 1.3 },
      { t: 2.4, x: 800, y: 540, scale: 1.32 },
      { t: 3.6, x: 800, y: 800, scale: 1.36 },
      { t: 6, x: 800, y: 810, scale: 1.38 },
    ] : [
      { t: 0, x: 960, y: 395, scale: 1.55 },
      { t: 2.4, x: 960, y: 410, scale: 1.58 },
      { t: 3.6, x: 960, y: 877, scale: 1.64 },
      { t: 6, x: 960, y: 877, scale: 1.66 },
    ]}>
      <div style={{ position: "absolute", left: LEFT, top: 110, width: WIDTH, color: "var(--ct-fg)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
          <SegmentedDonut segments={SEGMENTS} size={40} />
          <div>
            <div style={{ fontSize: 18, fontWeight: 600 }}>{PLAN.title}</div>
            <PlanStatusBreakdown counts={COUNTS} />
          </div>
        </div>
        <div style={{ display: "flex", alignItems: "baseline", gap: 12, margin: "22px 0 14px" }}>
          <span style={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: 17, color: "var(--ct-muted)" }}>{PRODUCT_TASK.key}</span>
          <span style={{ fontSize: 26, fontWeight: 650 }}>{PRODUCT_TASK.name}</span>
        </div>
        <DetailContent planId={PLAN_ID} tabs={PLAN_TABS} onOpenTab={open} onOpenTask={open} onSectionClick={open} kind="task" plan={PLAN} task={PRODUCT_TASK} />
      </div>
      <Spotlight t={t} keys={[
        { t: 0.2, rect: OWNER, dim: 0 },
        { t: 0.8, rect: OWNER },
        { t: 2.5, rect: OWNER },
        { t: 3.0, rect: OWNER, dim: 0 },
        { t: 3.9, rect: PROOF, dim: 0 },
        { t: 4.4, rect: PROOF },
      ]} />
    </Camera>
  );
}

const planBoard: Scene = {
  id: "plan-board",
  duration: 6,
  render: (t) => <PlanBoard t={t} />,
};

export default planBoard;
