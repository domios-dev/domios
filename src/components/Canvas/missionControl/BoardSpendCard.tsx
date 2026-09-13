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

/** Limits are counted per provider account on this machine, never for the team. */
const LIMITS_SCOPE_LABEL = "machine";

function shortProviderName(label: string): string {
  return label.split(/\s+/)[0] || label;
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
        {hasLimits && (
          <ul
            data-testid="mc-board-spend-limits"
            className="mc-board-spend-limits"
            role="list"
            aria-label={`Weekly limits, ${SPEND_SCOPE_CAPTION}`}
          >
            <li data-testid="mc-board-spend-limits-scope" className="mc-board-spend-limits-scope" aria-hidden="true">
              {LIMITS_SCOPE_LABEL}
            </li>
            {reading.groups.map((group) => (
              <li
                key={group.key}
                data-testid="mc-board-spend-provider"
                data-provider={group.key}
                className="mc-board-spend-limit"
                title={group.label}
              >
                <span aria-hidden="true" className="mc-board-swatch" data-tone={group.tone} />
                <span className="mc-board-spend-limit-name">{shortProviderName(group.label)}</span>
                {group.windows.map((window) => (
                  <span
                    key={window.key}
                    data-testid="mc-board-spend-gauge"
                    data-window-key={window.key}
                    data-variant={window.variant}
                    className="mc-board-spend-window"
                    title={`${SPEND_WINDOW_LABELS[window.variant]} limit used`}
                  >
                    {window.variant === "session" && `${SPEND_WINDOW_LABELS.session} `}
                    <b data-testid="mc-board-spend-window-pct" className={WINDOW_TONE[window.state]}>
                      <AnimatedNumber value={window.pct} format={formatWholePct} />
                    </b>
                  </span>
                ))}
              </li>
            ))}
          </ul>
        )}
      </BoardPanelSection>
    </BoardPanel>
  );
}
