import type { AllowanceDay, AllowanceWeeks } from "./allowanceWeek";
import { AnimatedNumber } from "./AnimatedNumber";
import { BoardPanel, BoardPanelSection } from "./BoardPanel";
import {
  formatCompact,
  formatCount,
  formatSharePct,
  SPEND_SCOPE_CAPTION,
  SPEND_SHARE_COLUMN,
  SPEND_TOKEN_COLUMNS,
  SPEND_WINDOW_LABELS,
  spendReading,
  type SpendProviderGroup,
  type SpendTokens,
  type SpendWindow,
} from "./missionControlBoardModel";
import type { MissionControlSpend } from "./missionControlState";

const SPEND_COLUMNS = [SPEND_SHARE_COLUMN, ...SPEND_TOKEN_COLUMNS];

const WINDOW_TONE: Record<SpendWindow["state"], string | undefined> = {
  neutral: undefined,
  warn: "mc-board-warn",
  danger: "mc-board-danger",
};

function formatWholePct(value: number): string {
  return `${Math.round(value)}%`;
}

// Rows are laid out with CSS grid, which drops native table semantics in some
// engines, so every table element restates its role explicitly.
export function SpendCard({ spend, weeks = {} }: { spend: MissionControlSpend; weeks?: AllowanceWeeks }) {
  const reading = spendReading(spend);
  return (
    <BoardPanel
      testId="mc-board-spend"
      title="Spend"
      className="mc-board-spend"
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
            <table
              data-testid="mc-board-spend-table"
              className="mc-board-spend-table"
              role="table"
              aria-label="Spend by provider and model"
            >
              <thead role="rowgroup">
                <tr role="row" data-testid="mc-board-spend-columns" className="mc-board-spend-row mc-board-spend-columns">
                  <td role="cell" aria-hidden="true" />
                  {SPEND_COLUMNS.map((column) => (
                    <th
                      key={column.key}
                      scope="col"
                      role="columnheader"
                      aria-label={column.label}
                      className="mc-board-spend-num"
                    >
                      <span aria-hidden="true" className="mc-board-spend-label-long">
                        {column.label}
                      </span>
                      <span aria-hidden="true" className="mc-board-spend-label-short">
                        {column.short}
                      </span>
                    </th>
                  ))}
                </tr>
              </thead>
              {reading.groups.map((group) => (
                <ProviderGroup key={group.key} group={group} week={weeks[group.key] ?? []} />
              ))}
            </table>
          </>
        )}
      </BoardPanelSection>
    </BoardPanel>
  );
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

function ProviderGroup({ group, week }: { group: SpendProviderGroup; week: AllowanceDay[] }) {
  return (
    <tbody role="rowgroup" data-testid="mc-board-spend-provider" data-provider={group.key}>
      <tr role="row" data-testid="mc-board-spend-group" className="mc-board-spend-row mc-board-spend-group">
        <th scope="rowgroup" role="rowheader" className="mc-board-spend-name">
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
        </th>
        <FigureCells sharePct={group.measured ? group.sharePct : null} tokens={group.measured ? group.tokens : null} />
      </tr>
      {group.models.map((model) => (
        <tr
          key={model.key}
          role="row"
          data-testid="mc-board-spend-model"
          data-model-key={model.key}
          data-unassigned={model.unassigned ? "true" : "false"}
          className="mc-board-spend-row mc-board-spend-model"
        >
          <th scope="row" role="rowheader" className="mc-board-spend-name mc-board-spend-model-name">
            {model.label}
          </th>
          <FigureCells sharePct={model.sharePct} tokens={model.tokens} />
        </tr>
      ))}
    </tbody>
  );
}

/** A provider with no measured rows leaves its cells empty rather than showing zeros. */
function FigureCells({ sharePct, tokens }: { sharePct: number | null; tokens: SpendTokens | null }) {
  return (
    <>
      <td role="cell" className="mc-board-spend-num mc-board-spend-share">
        {sharePct == null ? null : formatSharePct(sharePct)}
      </td>
      {SPEND_TOKEN_COLUMNS.map((column) => (
        <td key={column.key} role="cell" data-figure={column.key} className="mc-board-spend-num">
          {tokens && (
            <AnimatedNumber
              value={tokens[column.key]}
              format={column.key === "turns" ? formatCount : formatCompact}
            />
          )}
        </td>
      ))}
    </>
  );
}
