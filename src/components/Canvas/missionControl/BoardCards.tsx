import type { ReactNode } from "react";
import {
  areaReading,
  decisionsWaitingReading,
  formatAgeMs,
  formatCompact,
  formatCount,
  longestWaitReading,
  notRecordingCopy,
  taskStatusBreakdown,
} from "./missionControlBoardModel";
import { AnimatedNumber } from "./AnimatedNumber";
import type {
  AreaGrouping,
  BounceSummary,
  HeldForDecision,
  MissionControlKpi,
  PlanTaskCounts,
} from "./missionControlState";

export { SpendCard } from "./BoardSpendCard";

interface FigureCardProps {
  testId: string;
  className?: string;
  title: string;
  amount: ReactNode;
  amountTone?: "plain" | "warn";
  caption: ReactNode;
  detail?: ReactNode;
}

function FigureCard({ testId, className, title, amount, amountTone = "plain", caption, detail }: FigureCardProps) {
  return (
    <section
      data-testid={testId}
      className={`ct-panel-card mc-board-card mc-board-figure-card${className ? ` ${className}` : ""}`}
    >
      <h2>{title}</h2>
      <div data-testid={`${testId}-reading`} className="mc-board-reading">
        <div className="mc-board-figure">
          <div
            data-testid={`${testId}-amount`}
            className={`mc-board-amount${amountTone === "warn" ? " mc-board-amount-warn" : ""}`}
          >
            {amount}
          </div>
          <div className="mc-board-caption">{caption}</div>
        </div>
        {detail != null && <div className="mc-board-detail">{detail}</div>}
      </div>
    </section>
  );
}

export function TokensCard({ kpi }: { kpi: MissionControlKpi }) {
  const spend = kpi.spend;
  return (
    <FigureCard
      testId="mc-board-tokens"
      title="Tokens"
      amount={spend ? <AnimatedNumber value={spend.tokens.output} format={formatCompact} /> : "—"}
      caption={
        spend
          ? "output tokens on this plan"
          : notRecordingCopy(kpi.not_recording_reason, kpi.enable_setting)
      }
      detail={
        spend ? (
          <>
            <b><AnimatedNumber value={spend.tokens.input} format={formatCompact} /></b> input · <b><AnimatedNumber value={spend.tokens.uncached_input} format={formatCompact} /></b>{" "}
            of it uncached · <b><AnimatedNumber value={spend.tokens.total} format={formatCompact} /></b> in total
          </>
        ) : null
      }
    />
  );
}

export function TurnsCard({ kpi }: { kpi: MissionControlKpi }) {
  const spend = kpi.spend;
  return (
    <FigureCard
      testId="mc-board-turns"
      title="Turns"
      amount={spend ? <AnimatedNumber value={spend.turns} format={formatCount} /> : "—"}
      caption={
        spend
          ? "model turns on this plan"
          : notRecordingCopy(kpi.not_recording_reason, kpi.enable_setting)
      }
      detail={
        spend ? (
          <>
            <b><AnimatedNumber value={spend.events} format={formatCount} /></b> ledger events · <b><AnimatedNumber value={spend.bytes} format={formatCompact} /></b> bytes
          </>
        ) : null
      }
    />
  );
}

export function BouncesCard({ bounces }: { bounces: BounceSummary }) {
  return (
    <FigureCard
      testId="mc-board-bounces"
      title="Bounces"
      amount={<AnimatedNumber value={bounces.tasks_on_second_attempt} format={formatCount} />}
      amountTone={bounces.tasks_on_second_attempt > 0 ? "warn" : "plain"}
      caption="tasks on a second attempt"
    />
  );
}

export function HeldCard({ held }: { held: HeldForDecision }) {
  const count = held.held_reviews.length;
  return (
    <FigureCard
      testId="mc-board-held"
      title="Held for a decision"
      amount={<AnimatedNumber value={count} format={formatCount} />}
      amountTone={count > 0 ? "warn" : "plain"}
      caption="reviews held"
    />
  );
}

export function DecisionsWaitingCard({ held }: { held: HeldForDecision }) {
  const reading = decisionsWaitingReading(held);
  const gateLabel = reading.gateCount === 1 ? "gate open" : "gates open";
  const taskLabel = reading.blockedTaskCount === 1 ? "task blocked" : "tasks blocked";
  const caption =
    reading.gateCount === 0
      ? "no open gates"
      : `${gateLabel} · ${reading.oldestAgeMs == null ? "age unavailable" : `oldest ${formatAgeMs(reading.oldestAgeMs)}`} · ${formatCount(reading.blockedTaskCount)} ${taskLabel}`;
  return (
    <FigureCard
      testId="mc-board-decisions-waiting"
      title="Decisions waiting"
      amount={<AnimatedNumber value={reading.gateCount} format={formatCount} />}
      caption={caption}
    />
  );
}

export function LongestWaitCard({ held }: { held: HeldForDecision }) {
  const reading = longestWaitReading(held);
  return (
    <FigureCard
      testId="mc-board-longest-wait"
      title="Longest wait"
      amount={reading.ageMs == null ? "—" : formatAgeMs(reading.ageMs)}
      caption={reading.ageMs == null ? "nothing held" : reading.taskName ?? "task name unavailable"}
    />
  );
}

export function DoneFirstTryCard({ bounces }: { bounces: BounceSummary }) {
  const attemptLabel = bounces.total_attempts === 1 ? "attempt" : "attempts";
  return (
    <FigureCard
      testId="mc-board-done-first-try"
      title="Done first try"
      amount={<AnimatedNumber value={bounces.first_try_done} format={formatCount} />}
      caption={`${formatCount(bounces.total_attempts)} ${attemptLabel} · finished first try`}
    />
  );
}

export function AreasCard({ areas }: { areas: AreaGrouping }) {
  const reading = areaReading(areas);
  return (
    <section data-testid="mc-board-areas" className="ct-panel-card mc-board-card">
      <div className="mc-board-card-head">
        <h2>{reading.title}</h2>
        <span className="mc-board-caption">{reading.caption}</span>
      </div>
      {reading.rows.length === 0 ? (
        <p className="mc-board-note">No tasks to group.</p>
      ) : (
        <div className="mc-board-kv-list">
          {reading.rows.map((row) => (
            <div key={row.key} data-testid="mc-board-area-row" className="mc-board-kv">
              <span className="mc-board-kv-key">{row.label}</span>
              <span className="mc-board-kv-value">
                <AnimatedNumber value={row.taskCount} format={formatCount} />
              </span>
              <span className="mc-board-kv-share">
                <AnimatedNumber value={row.sharePct} />%
              </span>
            </div>
          ))}
        </div>
      )}
      {reading.declared && reading.untagged > 0 && (
        <p data-testid="mc-board-area-untagged" className="mc-board-detail">
          <b><AnimatedNumber value={reading.untagged} format={formatCount} /></b> untagged tasks
        </p>
      )}
    </section>
  );
}

export function TaskCountsCard({ counts }: { counts: PlanTaskCounts }) {
  const statuses = taskStatusBreakdown(counts);
  return (
    <FigureCard
      testId="mc-board-tasks"
      className="mc-board-card-tasks"
      title="Tasks"
      amount={<AnimatedNumber value={counts.total} format={formatCount} />}
      caption="total tasks"
      detail={
        <div data-testid="mc-board-task-statuses" className="mc-board-status-list" role="list">
          {statuses.map((status) => (
            <div
              key={status.key}
              data-testid={`mc-board-task-status-${status.key}`}
              data-zero={status.count === 0 ? "true" : "false"}
              className="mc-board-status"
              role="listitem"
            >
              <span className="mc-board-status-label">{status.label}</span>
              <b data-testid={`mc-board-task-status-${status.key}-count`}>
                <AnimatedNumber value={status.count} format={formatCount} />
              </b>
            </div>
          ))}
        </div>
      }
    />
  );
}
