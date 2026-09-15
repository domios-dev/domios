import { useId, useState } from "react";
import { AnimatedNumber } from "./AnimatedNumber";
import { BoardPanel, BoardPanelSection, ShowAll } from "./BoardPanel";
import { SpendChart } from "./BoardSpendChart";
import { formatCompact, formatCount, formatSharePct } from "./missionControlBoardModel";
import {
  TEAM_SPEND_DAYS,
  teamPlanShares,
  teamSpendNote,
  teamSpendTotals,
  teamTokensChart,
  type TeamSpend,
} from "./teamSpend";

export const SPEND_PERIOD_CAPTION = `last ${TEAM_SPEND_DAYS} days`;

const PLAN_SHARES_PREVIEW = 3;

/** What the team spent over the period in tokens, split by model on request; allowance percentages live in Analytics. */
export function SpendCard({ teamSpend = null }: { teamSpend?: TeamSpend | null }) {
  const [byModel, setByModel] = useState(false);

  return (
    <BoardPanel
      testId="mc-board-spend"
      title="Spend"
      className="mc-board-spend-panel"
      meta={
        teamSpend ? (
          <span className="mc-board-spend-head">
            <button
              type="button"
              data-testid="mc-board-spend-breakdown"
              className="mc-board-segment mc-board-segment-solo"
              aria-pressed={byModel}
              onClick={() => setByModel((value) => !value)}
            >
              by model
            </button>
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
        ) : (
          <TokensReading spend={teamSpend} byModel={byModel} />
        )}
      </BoardPanelSection>
    </BoardPanel>
  );
}

function TokensReading({ spend, byModel }: { spend: TeamSpend; byModel: boolean }) {
  const totals = teamSpendTotals(spend);
  const shares = teamPlanShares(spend);
  const [showAll, setShowAll] = useState(false);
  const listId = useId();
  const note = teamSpendNote(spend);
  if (note) {
    return (
      <p data-testid="mc-board-spend-not-recording" className="mc-board-note">
        {note}
      </p>
    );
  }
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
