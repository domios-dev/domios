import type { AllowanceWeeks } from "./allowanceWeek";
import { AnimatedNumber } from "./AnimatedNumber";
import { BoardPanel, BoardPanelSection } from "./BoardPanel";
import { SpendChart } from "./BoardSpendChart";
import {
  SPEND_SCOPE_CAPTION,
  SPEND_WINDOW_LABELS,
  spendReading,
  type SpendWindow,
} from "./missionControlBoardModel";
import type { MissionControlSpend } from "./missionControlState";
import { allowanceChartSource, combineChartSources } from "./spendChart";
import { TEAM_SCOPE_CAPTION, teamTokensChartSource, type TeamSpend } from "./teamSpend";

const WINDOW_TONE: Record<SpendWindow["state"], string | undefined> = {
  neutral: undefined,
  warn: "mc-board-warn",
  danger: "mc-board-danger",
};

function formatWholePct(value: number): string {
  return `${Math.round(value)}%`;
}

/** Spend over the week as a chart, then each provider's weekly and session allowance used so far. */
export function SpendCard({
  spend,
  weeks = {},
  teamSpend = null,
}: {
  spend: MissionControlSpend;
  weeks?: AllowanceWeeks;
  teamSpend?: TeamSpend | null;
}) {
  const reading = spendReading(spend);
  const chart = combineChartSources(
    teamSpend ? teamTokensChartSource(teamSpend) : null,
    allowanceChartSource(weeks, reading.groups),
  );
  const hasLimits = reading.groups.length > 0;
  return (
    <BoardPanel
      testId="mc-board-spend"
      title="Spend"
      meta={<span data-testid="mc-board-spend-scope">{teamSpend ? TEAM_SCOPE_CAPTION : SPEND_SCOPE_CAPTION}</span>}
    >
      <BoardPanelSection>
        {chart.hasData && <SpendChart source={chart} />}
        {reading.emptyCopy && !chart.hasData && (
          <p data-testid="mc-board-spend-empty" className="mc-board-note">
            {reading.emptyCopy}
          </p>
        )}
        {hasLimits && reading.note && (
          <p data-testid="mc-board-spend-units" className="mc-board-caption">
            {reading.note}
          </p>
        )}
        {hasLimits && teamSpend && (
          <h3 data-testid="mc-board-spend-limits-label" className="mc-board-sub-label">
            Limits, {SPEND_SCOPE_CAPTION}
          </h3>
        )}
        {hasLimits && (
          <ul className="mc-board-spend-providers" role="list">
            {reading.groups.map((group) => (
              <li
                key={group.key}
                data-testid="mc-board-spend-provider"
                data-provider={group.key}
                className="mc-board-spend-provider"
              >
                <span className="mc-board-spend-provider-name">
                  <span aria-hidden="true" className="mc-board-swatch" data-tone={group.tone} />
                  <span>{group.label}</span>
                </span>
                {group.windows.length > 0 && (
                  <span className="mc-board-spend-windows">
                    {group.windows.map((window) => (
                      <span
                        key={window.key}
                        data-testid="mc-board-spend-gauge"
                        data-window-key={window.key}
                        className="mc-board-spend-window"
                      >
                        {SPEND_WINDOW_LABELS[window.variant]}{" "}
                        <b data-testid="mc-board-spend-window-pct" className={WINDOW_TONE[window.state]}>
                          <AnimatedNumber value={window.pct} format={formatWholePct} />
                        </b>
                      </span>
                    ))}
                  </span>
                )}
              </li>
            ))}
          </ul>
        )}
      </BoardPanelSection>
    </BoardPanel>
  );
}
