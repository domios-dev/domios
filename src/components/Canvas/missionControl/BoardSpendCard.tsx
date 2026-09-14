import { useId, useState } from "react";
import { AnimatedNumber } from "./AnimatedNumber";
import { BoardPanel, BoardPanelSection, ShowAll } from "./BoardPanel";
import { SpendChart } from "./BoardSpendChart";
import { formatCompact, formatCount, formatSharePct } from "./missionControlBoardModel";
import {
  TEAM_SPEND_DAYS,
  teamPlanShares,
  teamSpendTotals,
  teamTokensChart,
  teamWeeklyChart,
  teamWeeklyUsed,
  type TeamSpend,
} from "./teamSpend";

export const SPEND_PERIOD_CAPTION = `last ${TEAM_SPEND_DAYS} days`;

export type SpendUnits = "tokens" | "weekly_points";

export const SPEND_UNIT_OPTIONS: ReadonlyArray<{ key: SpendUnits; label: string }> = [
  { key: "tokens", label: "tokens" },
  { key: "weekly_points", label: "weekly %" },
];

/** Providers report weekly allowance per account, so it cannot be split by model. */
export const BREAKDOWN_UNAVAILABLE = "weekly allowance is not reported per model";

const PLAN_SHARES_PREVIEW = 3;

/** What the team spent over the period, read in tokens or in weekly allowance, split by model on request. */
export function SpendCard({
  teamSpend = null,
  teamWeekly = null,
}: {
  teamSpend?: TeamSpend | null;
  teamWeekly?: TeamSpend | null;
}) {
  const [units, setUnits] = useState<SpendUnits>("tokens");
  const [byModel, setByModel] = useState(false);
  const reasonId = useId();
  const weekly = units === "weekly_points";

  return (
    <BoardPanel
      testId="mc-board-spend"
      title="Spend"
      className="mc-board-spend-panel"
      meta={
        teamSpend ? (
          <span className="mc-board-spend-head">
            <span role="group" aria-label="Units" className="mc-board-segmented">
              {SPEND_UNIT_OPTIONS.map((option) => (
                <button
                  key={option.key}
                  type="button"
                  data-testid={`mc-board-spend-units-${option.key}`}
                  className="mc-board-segment"
                  aria-pressed={units === option.key}
                  onClick={() => setUnits(option.key)}
                >
                  {option.label}
                </button>
              ))}
            </span>
            <button
              type="button"
              data-testid="mc-board-spend-breakdown"
              className="mc-board-segment mc-board-segment-solo"
              aria-pressed={!weekly && byModel}
              aria-disabled={weekly ? true : undefined}
              aria-describedby={weekly ? reasonId : undefined}
              title={weekly ? BREAKDOWN_UNAVAILABLE : undefined}
              onClick={() => {
                if (!weekly) setByModel((value) => !value);
              }}
            >
              by model
            </button>
            <span id={reasonId} hidden>
              {BREAKDOWN_UNAVAILABLE}
            </span>
            <span data-testid="mc-board-spend-period">{SPEND_PERIOD_CAPTION}</span>
          </span>
        ) : undefined
      }
    >
      <BoardPanelSection testId="mc-board-spend-body">
        {!teamSpend ? (
          <p data-testid="mc-board-spend-empty" className="mc-board-note">
            no data
          </p>
        ) : weekly ? (
          <WeeklyReading weekly={teamWeekly} />
        ) : (
          <TokensReading spend={teamSpend} byModel={byModel} />
        )}
      </BoardPanelSection>
    </BoardPanel>
  );
}

function WeeklyReading({ weekly }: { weekly: TeamSpend | null }) {
  const used = weekly ? teamWeeklyUsed(weekly) : [];
  if (!weekly || used.length === 0) {
    return (
      <p data-testid="mc-board-spend-weekly-empty" className="mc-board-note">
        no data
      </p>
    );
  }
  return (
    <>
      <dl data-testid="mc-board-spend-weekly-used" className="mc-board-pairs mc-board-spend-totals">
        {used.map((entry) => (
          <div key={entry.key} className="mc-board-pair">
            <dt>{entry.label}</dt>
            <dd>
              <b data-testid="mc-board-spend-weekly-pct">
                <AnimatedNumber value={entry.pct} format={formatSharePct} />
              </b>
            </dd>
          </div>
        ))}
      </dl>
      <SpendChart data={teamWeeklyChart(weekly)} label="weekly points" format={formatSharePct} />
    </>
  );
}

function TokensReading({ spend, byModel }: { spend: TeamSpend; byModel: boolean }) {
  const totals = teamSpendTotals(spend);
  const shares = teamPlanShares(spend);
  const [showAll, setShowAll] = useState(false);
  const listId = useId();
  const visibleShares = showAll ? shares : shares.slice(0, PLAN_SHARES_PREVIEW);
  return (
    <>
      <dl data-testid="mc-board-spend-totals" className="mc-board-pairs mc-board-spend-totals">
        <div className="mc-board-pair">
          <dt>new input</dt>
          <dd>
            <b data-testid="mc-board-spend-new-input">
              <AnimatedNumber value={totals.newInput} format={formatCompact} />
            </b>
          </dd>
        </div>
        <div className="mc-board-pair">
          <dt>generated</dt>
          <dd>
            <b data-testid="mc-board-spend-generated">
              <AnimatedNumber value={totals.generated} format={formatCompact} />
            </b>
          </dd>
        </div>
        <div className="mc-board-pair">
          <dt>turns</dt>
          <dd>
            <b data-testid="mc-board-spend-turns">
              <AnimatedNumber value={totals.turns} format={formatCount} />
            </b>
          </dd>
        </div>
      </dl>
      <SpendChart data={teamTokensChart(spend, byModel)} label="new input" format={formatCompact} />
      {shares.length > 1 && (
        <>
          <ul id={listId} data-testid="mc-board-spend-plan-shares" className="mc-board-plan-split" role="list">
            {visibleShares.map((share) => (
              <li
                key={share.planId}
                data-testid="mc-board-spend-plan-share"
                className="mc-board-plan-split-row"
                title={share.title}
              >
                <span className="mc-board-plan-split-title">{share.title}</span>
                <span className="mc-board-plan-split-share">{formatSharePct(share.sharePct)}</span>
              </li>
            ))}
          </ul>
          <ShowAll
            total={shares.length}
            limit={PLAN_SHARES_PREVIEW}
            open={showAll}
            controls={listId}
            testId="mc-board-spend-plan-shares-more"
            onToggle={() => setShowAll((value) => !value)}
          />
        </>
      )}
    </>
  );
}
