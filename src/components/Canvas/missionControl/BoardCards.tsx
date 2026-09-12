import type { ReactNode } from "react";
import { UsageGauge } from "../../Usage/UsageGauge";
import {
  areaReading,
  BOARD_RUNWAY_COLUMNS,
  formatAgeMs,
  formatCompact,
  formatCount,
  notRecordingCopy,
  runwayGauges,
  type BoardLayout,
} from "./missionControlBoardModel";
import type {
  AreaGrouping,
  BounceSummary,
  HeldForDecision,
  MissionControlKpi,
  RunwaySection,
} from "./missionControlState";

interface FigureCardProps {
  testId: string;
  title: string;
  amount: string;
  amountTone?: "plain" | "warn";
  caption: string;
  detail: ReactNode;
  source: string;
}

function FigureCard({ testId, title, amount, amountTone = "plain", caption, detail, source }: FigureCardProps) {
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
      <p className="mc-board-detail">{detail}</p>
      <p className="mc-board-source">{source}</p>
    </section>
  );
}

export function TokensCard({ kpi }: { kpi: MissionControlKpi }) {
  const spend = kpi.spend;
  return (
    <FigureCard
      testId="mc-board-tokens"
      title="Tokens"
      amount={spend ? formatCompact(spend.tokens.output) : "—"}
      caption={spend ? "output tokens on this plan" : notRecordingCopy(kpi.not_recording_reason)}
      detail={
        spend ? (
          <>
            <b>{formatCompact(spend.tokens.input)}</b> input · <b>{formatCompact(spend.tokens.uncached_input)}</b>{" "}
            of it uncached · <b>{formatCompact(spend.tokens.total)}</b> in total
          </>
        ) : (
          "No token figure is shown while the ledger is not recording."
        )
      }
      source="KPI ledger, the rows attributed to this plan"
    />
  );
}

export function TurnsCard({ kpi }: { kpi: MissionControlKpi }) {
  const spend = kpi.spend;
  return (
    <FigureCard
      testId="mc-board-turns"
      title="Turns"
      amount={spend ? formatCount(spend.turns) : "—"}
      caption={spend ? "model turns on this plan" : notRecordingCopy(kpi.not_recording_reason)}
      detail={
        spend ? (
          <>
            <b>{formatCount(spend.events)}</b> ledger events · <b>{formatCompact(spend.bytes)}</b> bytes
          </>
        ) : (
          "No turn figure is shown while the ledger is not recording."
        )
      }
      source="KPI ledger, the rows attributed to this plan"
    />
  );
}

export function BouncesCard({ bounces }: { bounces: BounceSummary }) {
  return (
    <FigureCard
      testId="mc-board-bounces"
      title="Bounces"
      amount={formatCount(bounces.tasks_on_second_attempt)}
      amountTone={bounces.tasks_on_second_attempt > 0 ? "warn" : "plain"}
      caption="tasks on a second attempt"
      detail={
        <>
          <b>{formatCount(bounces.total_attempts)}</b> attempts · <b>{formatCount(bounces.first_try_done)}</b>{" "}
          finished on the first · <b>{formatCount(bounces.labelled_bounced)}</b> labelled bounced
        </>
      }
      source="stored execution attempts and status labels"
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
      amount={formatCount(count)}
      amountTone={count > 0 ? "warn" : "plain"}
      caption={
        oldest == null
          ? "reviews held"
          : `reviews held, the oldest ${formatAgeMs(oldest)} since its last verdict`
      }
      detail={
        <>
          <b>{formatCount(held.open_gates.length)}</b> decision gates open ·{" "}
          <b>{formatCount(held.verdicts_on_held)}</b> verdicts on the held tasks
        </>
      }
      source="the stored review-held label and the plan's open gates"
    />
  );
}

export function AreasCard({ areas }: { areas: AreaGrouping }) {
  const reading = areaReading(areas);
  return (
    <section data-testid="mc-board-areas" className="ct-panel-card mc-board-card">
      <div className="mc-board-card-head">
        <h2>Spend by area</h2>
        <span className="mc-board-caption">
          {reading.declared
            ? `one ${reading.facetLabel ?? "area"} per task, from the list the plan declares`
            : "no area facet declared"}
        </span>
      </div>
      {reading.rows.length === 0 ? (
        <p className="mc-board-note">This plan has no tasks to group.</p>
      ) : (
        <div className="mc-board-kv-list">
          {reading.rows.map((row) => (
            <div key={row.key} data-testid="mc-board-area-row" className="mc-board-kv">
              <span className="mc-board-kv-key">
                {row.label}
                {row.qualifier && <i className="mc-board-kv-qualifier">{row.qualifier}</i>}
              </span>
              <span className="mc-board-kv-value">{formatCount(row.taskCount)}</span>
              <span className="mc-board-kv-share">{row.sharePct}%</span>
            </div>
          ))}
        </div>
      )}
      {reading.declared && reading.untagged > 0 && (
        <p data-testid="mc-board-area-untagged" className="mc-board-detail">
          <b>{formatCount(reading.untagged)}</b> tasks carry no area tag and are not placed under one.
        </p>
      )}
      {reading.noFacetMessage && (
        <p data-testid="mc-board-area-no-facet" className="mc-board-detail">
          {reading.noFacetMessage}
        </p>
      )}
      <p className="mc-board-source">
        tasks per group; the ledger does not attribute tokens to an area, so none is shown
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
      <div className="mc-board-card-head">
        <h2>Runway</h2>
        <span className="mc-board-caption">one snapshot per provider, each with its own reset</span>
      </div>
      {gauges.length === 0 ? (
        <p data-testid="mc-board-runway-empty" className="mc-board-note">
          No provider reports usage on this machine.
        </p>
      ) : (
        <div
          data-testid="mc-board-runway-grid"
          className="mc-board-runway-grid"
          style={{ gridTemplateColumns: BOARD_RUNWAY_COLUMNS[layout] }}
        >
          {gauges.map((gauge) => (
            <div key={gauge.key} data-testid="mc-board-runway-gauge" data-runway-key={gauge.key}>
              <UsageGauge
                label={gauge.label}
                variant={gauge.variant}
                pct={gauge.pct}
                resetsAt={gauge.resetsAt}
                state={gauge.state}
                now={now}
              />
              {gauge.staleReason && (
                <span className="mc-board-source">{gauge.staleReason}</span>
              )}
            </div>
          ))}
        </div>
      )}
      <p className="mc-board-source">
        usage snapshots, the latest parsed row per provider; 60% and 85% are marked on every bar
      </p>
    </section>
  );
}
