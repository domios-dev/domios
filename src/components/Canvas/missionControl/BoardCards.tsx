import type { ReactNode } from "react";
import { UsageGauge } from "../../Usage/UsageGauge";
import {
  areaReading,
  BOARD_RUNWAY_COLUMNS,
  formatAgeMs,
  formatCompact,
  formatCount,
  notRecordingCopy,
  notRecordingDetail,
  runwayGauges,
  taskStatusBreakdown,
  type BoardLayout,
} from "./missionControlBoardModel";
import { AnimatedNumber, useAnimatedNumber } from "./AnimatedNumber";
import type {
  AreaGrouping,
  BounceSummary,
  HeldForDecision,
  MissionControlKpi,
  PlanTaskCounts,
  RunwaySection,
} from "./missionControlState";

interface FigureCardProps {
  testId: string;
  title: string;
  amount: ReactNode;
  amountTone?: "plain" | "warn";
  caption: ReactNode;
  detail: ReactNode;
}

function FigureCard({ testId, title, amount, amountTone = "plain", caption, detail }: FigureCardProps) {
  return (
    <section data-testid={testId} className="ct-panel-card mc-board-card">
      <h2>{title}</h2>
      <div>
        <div
          data-testid={`${testId}-amount`}
          className={`mc-board-amount${amountTone === "warn" ? " mc-board-amount-warn" : ""}`}
        >
          {amount}
        </div>
        <div className="mc-board-caption">{caption}</div>
      </div>
      <div className="mc-board-detail">{detail}</div>
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
      caption={spend ? "output tokens on this plan" : notRecordingCopy(kpi.not_recording_reason)}
      detail={
        spend ? (
          <>
            <b><AnimatedNumber value={spend.tokens.input} format={formatCompact} /></b> input · <b><AnimatedNumber value={spend.tokens.uncached_input} format={formatCompact} /></b>{" "}
            of it uncached · <b><AnimatedNumber value={spend.tokens.total} format={formatCompact} /></b> in total
          </>
        ) : (
          notRecordingDetail(kpi.not_recording_reason, kpi.enable_setting)
        )
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
      caption={spend ? "model turns on this plan" : notRecordingCopy(kpi.not_recording_reason)}
      detail={
        spend ? (
          <>
            <b><AnimatedNumber value={spend.events} format={formatCount} /></b> ledger events · <b><AnimatedNumber value={spend.bytes} format={formatCompact} /></b> bytes
          </>
        ) : (
          notRecordingDetail(kpi.not_recording_reason, kpi.enable_setting)
        )
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
      detail={
        <>
          <b><AnimatedNumber value={bounces.total_attempts} format={formatCount} /></b> attempts · <b><AnimatedNumber value={bounces.first_try_done} format={formatCount} /></b>{" "}
            finished on the first · <b><AnimatedNumber value={bounces.labelled_bounced} format={formatCount} /></b> labelled bounced
        </>
      }
    />
  );
}

export function HeldCard({ held }: { held: HeldForDecision }) {
  const count = held.held_reviews.length;
  const oldest = held.oldest_hold_age_ms;
  return (
    <FigureCard
      testId="mc-board-held"
      title="Held for a decision"
      amount={<AnimatedNumber value={count} format={formatCount} />}
      amountTone={count > 0 ? "warn" : "plain"}
      caption={
        oldest == null
          ? "reviews held"
          : `reviews held, the oldest ${formatAgeMs(oldest)} since its last verdict`
      }
      detail={
        <>
          <b><AnimatedNumber value={held.open_gates.length} format={formatCount} /></b> decision gates open ·{" "}
            <b><AnimatedNumber value={held.verdicts_on_held} format={formatCount} /></b> verdicts on the held tasks
        </>
      }
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
              <span className="mc-board-kv-key">
                {row.label}
                {row.qualifier && <i className="mc-board-kv-qualifier">{row.qualifier}</i>}
              </span>
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
      {reading.noFacetMessage && (
        <p data-testid="mc-board-area-no-facet" className="mc-board-detail">
          {reading.noFacetMessage}
        </p>
      )}
      <p data-testid="mc-board-area-tokens-relation" className="mc-board-detail mc-board-relation">
        Task counts only · Tokens plan total · no area token attribution
      </p>
    </section>
  );
}

export function RunwayCard({
  runway,
  layout,
  now,
}: {
  runway: RunwaySection;
  layout: BoardLayout;
  now: number;
}) {
  const gauges = runwayGauges(runway.providers);
  return (
    <section data-testid="mc-board-runway" className="ct-panel-card mc-board-card">
      <h2>Runway</h2>
      {gauges.length === 0 ? (
        <p data-testid="mc-board-runway-empty" className="mc-board-note">
          {runway.providers.length > 0
            ? "No complete allowance window available."
            : "No provider usage reported."}
        </p>
      ) : (
        <div
          data-testid="mc-board-runway-grid"
          className="mc-board-runway-grid"
          style={{ gridTemplateColumns: BOARD_RUNWAY_COLUMNS[layout] }}
        >
          {gauges.map((gauge) => (
            <RunwayGauge key={gauge.key} gauge={gauge} now={now} />
          ))}
        </div>
      )}
    </section>
  );
}

export function TaskCountsCard({ counts }: { counts: PlanTaskCounts }) {
  const statuses = taskStatusBreakdown(counts);
  return (
    <FigureCard
      testId="mc-board-tasks"
      title="Tasks"
      amount={<AnimatedNumber value={counts.total} format={formatCount} />}
      caption={<><AnimatedNumber value={counts.in_progress + counts.in_review} format={formatCount} /> active or in review</>}
      detail={
        <div data-testid="mc-board-task-statuses" className="mc-board-status-list" role="list">
          {statuses.map((status) => (
            <div key={status.key} data-testid={`mc-board-task-status-${status.key}`} className="mc-board-status" role="listitem">
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

function RunwayGauge({ gauge, now }: { gauge: ReturnType<typeof runwayGauges>[number]; now: number }) {
  const pct = useAnimatedNumber(gauge.pct);
  return (
    <div data-testid="mc-board-runway-gauge" data-runway-key={gauge.key} className="mc-board-runway-gauge">
      <UsageGauge
        label={gauge.label}
        variant={gauge.variant}
        pct={pct}
        resetsAt={gauge.resetsAt}
        state={gauge.state}
        now={now}
      />
    </div>
  );
}
