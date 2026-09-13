import type { AllowanceDay, AllowanceWeeks } from "./allowanceWeek";
import { AnimatedNumber } from "./AnimatedNumber";
import { BoardPanel, BoardPanelSection } from "./BoardPanel";
import {
  SPEND_SCOPE_CAPTION,
  SPEND_WINDOW_LABELS,
  spendReading,
  type SpendWindow,
} from "./missionControlBoardModel";
import type { MissionControlSpend } from "./missionControlState";

const WINDOW_TONE: Record<SpendWindow["state"], string | undefined> = {
  neutral: undefined,
  warn: "mc-board-warn",
  danger: "mc-board-danger",
};

function formatWholePct(value: number): string {
  return `${Math.round(value)}%`;
}

/** Seven bars, one per day of the weekly cycle, scaled to the busiest day. */
function AllowanceWeekBars({ provider, tone, days }: { provider: string; tone: number; days: AllowanceDay[] }) {
  const busiest = Math.max(5, ...days.map((day) => day.pct ?? 0));
  const lived = days.filter((day) => day.pct != null);
  const summary = lived.map((day) => `${day.label} ${Math.round(day.pct ?? 0)}%`).join(", ");
  return (
    <span
      data-testid="mc-board-spend-week"
      role="img"
      aria-label={`${provider} weekly allowance by day: ${summary}`}
      className="mc-board-spend-week"
      data-tone={tone}
    >
      {days.map((day) => (
        <span
          key={day.start}
          data-testid="mc-board-spend-day"
          className="mc-board-spend-day"
          data-today={day.today ? "true" : undefined}
          data-future={day.pct == null ? "true" : undefined}
          title={day.pct == null ? day.label : `${day.label} ${Math.round(day.pct)}%`}
          style={{ height: `${Math.round(((day.pct ?? 0) / busiest) * 100)}%` }}
        />
      ))}
    </span>
  );
}

/** Each provider's weekly allowance used so far, with the week drawn day by day. */
export function SpendCard({ spend, weeks = {} }: { spend: MissionControlSpend; weeks?: AllowanceWeeks }) {
  const reading = spendReading(spend);
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
            <ul className="mc-board-spend-providers" role="list">
              {reading.groups.map((group) => {
                const week = weeks[group.key] ?? [];
                return (
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
                    {week.length > 0 && <AllowanceWeekBars provider={group.label} tone={group.tone} days={week} />}
                  </li>
                );
              })}
            </ul>
          </>
        )}
      </BoardPanelSection>
    </BoardPanel>
  );
}
