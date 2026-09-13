import type { ReactNode } from "react";
import {
  areaReading,
  formatCompact,
  formatCount,
  needsYouItems,
  notRecordingCopy,
  progressReading,
} from "./missionControlBoardModel";
import { AnimatedNumber } from "./AnimatedNumber";
import { BoardPanel, BoardPanelSection, Facts } from "./BoardPanel";
import type {
  AreaGrouping,
  BounceSummary,
  HeldForDecision,
  MissionControlKpi,
  PlanTaskCounts,
} from "./missionControlState";

export { SpendCard } from "./BoardSpendCard";

const WARN_STATUSES = new Set(["blocked", "paused"]);

export function NeedsYouPanel({ held }: { held: HeldForDecision }) {
  const items = needsYouItems(held);
  if (items.length === 0) return null;
  return (
    <BoardPanel testId="mc-board-needs-you" title="Needs you">
      <div className="mc-board-needs">
        <span data-testid="mc-board-needs-you-count" className="mc-board-figure mc-board-warn">
          <AnimatedNumber value={items.length} format={formatCount} />
        </span>
        <ul className="mc-board-needs-list">
          {items.map((item) => (
            <li key={item.key} data-testid="mc-board-needs-you-item" data-kind={item.kind}>
              <p className="mc-board-needs-title">{item.title}</p>
              <Facts items={item.facts.map((fact, index) => ({ key: String(index), node: fact }))} />
            </li>
          ))}
        </ul>
      </div>
    </BoardPanel>
  );
}

export function ProgressPanel({
  counts,
  kpi,
  bounces,
  areas,
}: {
  counts: PlanTaskCounts;
  kpi: MissionControlKpi;
  bounces: BounceSummary;
  areas: AreaGrouping;
}) {
  const progress = progressReading(counts);
  return (
    <BoardPanel
      testId="mc-board-progress"
      title="Progress"
      meta={
        <>
          <span data-testid="mc-board-tasks-amount" className="mc-board-num">
            <AnimatedNumber value={progress.total} format={formatCount} />
          </span>
          {progress.total === 1 ? " task" : " tasks"}
        </>
      }
    >
      <BoardPanelSection testId="mc-board-task-statuses">
        {progress.active.length === 0 ? (
          <p className="mc-board-note">No tasks yet.</p>
        ) : (
          <ul className="mc-board-statuses" role="list">
            {progress.active.map((status) => (
              <li key={status.key} data-testid={`mc-board-task-status-${status.key}`} className="mc-board-status">
                <b
                  data-testid={`mc-board-task-status-${status.key}-count`}
                  className={`mc-board-figure${WARN_STATUSES.has(status.key) ? " mc-board-warn" : ""}`}
                >
                  <AnimatedNumber value={status.count} format={formatCount} />
                </b>
                <span>{status.label}</span>
              </li>
            ))}
          </ul>
        )}
      </BoardPanelSection>
      <PlanSpendSection kpi={kpi} />
      <QualitySection bounces={bounces} />
      <AreasSection areas={areas} />
    </BoardPanel>
  );
}

function Pair({
  label,
  testId,
  warn = false,
  unit,
  children,
}: {
  label: string;
  testId: string;
  warn?: boolean;
  unit?: string;
  children: ReactNode;
}) {
  return (
    <div className="mc-board-pair">
      <dt>{label}</dt>
      <dd>
        <b data-testid={testId} className={warn ? "mc-board-warn" : undefined}>
          {children}
        </b>
        {unit && ` ${unit}`}
      </dd>
    </div>
  );
}

function PlanSpendSection({ kpi }: { kpi: MissionControlKpi }) {
  const spend = kpi.spend;
  return (
    <BoardPanelSection testId="mc-board-plan-spend" label="This plan">
      {spend ? (
        <dl className="mc-board-pairs">
          <Pair label="new input" testId="mc-board-plan-new-input">
            <AnimatedNumber value={spend.tokens.uncached_input} format={formatCompact} />
          </Pair>
          <Pair label="turns" testId="mc-board-plan-turns">
            <AnimatedNumber value={spend.turns} format={formatCount} />
          </Pair>
        </dl>
      ) : (
        <p data-testid="mc-board-plan-not-recording" className="mc-board-caption">
          {notRecordingCopy(kpi.not_recording_reason, kpi.enable_setting)}
        </p>
      )}
    </BoardPanelSection>
  );
}

function QualitySection({ bounces }: { bounces: BounceSummary }) {
  const bounced = bounces.tasks_on_second_attempt;
  const attempts = bounces.total_attempts;
  return (
    <BoardPanelSection testId="mc-board-quality" label="Quality">
      <dl className="mc-board-pairs">
        <Pair label="bounced" testId="mc-board-bounces-amount" warn={bounced > 0}>
          <AnimatedNumber value={bounced} format={formatCount} />
        </Pair>
        <Pair label="first try" testId="mc-board-done-first-try-amount" unit={`of ${formatCount(attempts)}`}>
          <AnimatedNumber value={bounces.first_try_done} format={formatCount} />
        </Pair>
      </dl>
    </BoardPanelSection>
  );
}

function AreasSection({ areas }: { areas: AreaGrouping }) {
  const reading = areaReading(areas);
  return (
    <BoardPanelSection testId="mc-board-areas" label={reading.title}>
      {reading.rows.length === 0 ? (
        <p className="mc-board-note">No tasks to group.</p>
      ) : (
        <dl className="mc-board-kv-list">
          {reading.rows.map((row) => (
            <div
              key={row.key}
              data-testid="mc-board-area-row"
              data-untagged={row.untagged ? "true" : "false"}
              className="mc-board-kv"
            >
              <dt className="mc-board-kv-key">{row.label}</dt>
              <dd className="mc-board-kv-value">
                <AnimatedNumber value={row.taskCount} format={formatCount} />
              </dd>
            </div>
          ))}
        </dl>
      )}
    </BoardPanelSection>
  );
}
