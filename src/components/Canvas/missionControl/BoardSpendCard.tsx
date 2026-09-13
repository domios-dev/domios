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
import { allowanceChartSource } from "./spendChart";

const WINDOW_TONE: Record<SpendWindow["state"], string | undefined> = {
  neutral: undefined,
  warn: "mc-board-warn",
  danger: "mc-board-danger",
};

function formatWholePct(value: number): string {
  return `${Math.round(value)}%`;
}

/** Spend over the week as a chart, then each provider's weekly and session allowance used so far. */
export function SpendCard({ spend, weeks = {} }: { spend: MissionControlSpend; weeks?: AllowanceWeeks }) {
  const reading = spendReading(spend);
  const chart = allowanceChartSource(weeks, reading.groups);
  return (
    <BoardPanel
      testId="mc-board-spend"
      title="Spend"
      meta={<span data-testid="mc-board-spend-scope">{SPEND_SCOPE_CAPTION}</span>}
    >
      <BoardPanelSection>
        {reading.emptyCopy ? (
          <p data-testid="mc-board-spend-empty" className="mc-board-note">
            {reading.emptyCopy}
          </p>
        ) : (
          <>
            {reading.note && (
              <p data-testid="mc-board-spend-units" className="mc-board-caption">
                {reading.note}
              </p>
            )}
            {chart.hasData && <SpendChart source={chart} />}
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
          </>
        )}
      </BoardPanelSection>
    </BoardPanel>
  );
}
