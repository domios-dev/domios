import { useId } from "react";
import { UsageGauge } from "../../Usage/UsageGauge";
import { AnimatedNumber, useAnimatedNumber } from "./AnimatedNumber";
import {
  BOARD_SPEND_WINDOW_COLUMNS,
  formatCompact,
  formatCount,
  formatSharePct,
  SPEND_FIGURE_COLUMNS,
  SPEND_MODEL_COLUMN_LABEL,
  SPEND_SCOPE_CAPTION,
  SPEND_SHARE_COLUMN_LABEL,
  spendReading,
  type BoardLayout,
  type SpendProviderGroup,
  type SpendWindow,
} from "./missionControlBoardModel";
import type { MissionControlSpend } from "./missionControlState";

const SPEND_COLUMN_COUNT = 2 + SPEND_FIGURE_COLUMNS.length;

// Rows are laid out with CSS grid, which drops native table semantics in some
// engines, so every table element restates its role explicitly.
export function SpendCard({
  spend,
  layout,
  now,
}: {
  spend: MissionControlSpend;
  layout: BoardLayout;
  now: number;
}) {
  const reading = spendReading(spend);
  const headingId = useId();
  return (
    <section
      data-testid="mc-board-spend"
      className="ct-panel-card mc-board-card mc-board-spend"
      aria-labelledby={headingId}
    >
      <div className="mc-board-card-head">
        <h2 id={headingId}>Spend</h2>
        <span data-testid="mc-board-spend-scope" className="mc-board-caption">
          {SPEND_SCOPE_CAPTION}
        </span>
      </div>
      {reading.emptyCopy ? (
        <p data-testid="mc-board-spend-empty" className="mc-board-note">
          {reading.emptyCopy}
        </p>
      ) : (
        <>
          <p data-testid="mc-board-spend-units" className="mc-board-caption">
            {reading.note}
          </p>
          <table
            data-testid="mc-board-spend-table"
            className="mc-board-spend-table"
            role="table"
            aria-labelledby={headingId}
          >
            <thead role="rowgroup">
              <tr role="row" data-testid="mc-board-spend-columns" className="mc-board-spend-row mc-board-spend-columns">
                <th scope="col" role="columnheader">
                  {SPEND_MODEL_COLUMN_LABEL}
                </th>
                <th scope="col" role="columnheader" className="mc-board-spend-num">
                  {SPEND_SHARE_COLUMN_LABEL}
                </th>
                {SPEND_FIGURE_COLUMNS.map((column) => (
                  <th key={column.key} scope="col" role="columnheader" className="mc-board-spend-num">
                    {column.label}
                  </th>
                ))}
              </tr>
            </thead>
            {reading.groups.map((group) => (
              <SpendProvider key={group.key} group={group} layout={layout} now={now} />
            ))}
          </table>
        </>
      )}
    </section>
  );
}

function SpendProvider({
  group,
  layout,
  now,
}: {
  group: SpendProviderGroup;
  layout: BoardLayout;
  now: number;
}) {
  return (
    <tbody
      role="rowgroup"
      data-testid="mc-board-spend-provider"
      data-provider={group.key}
      className="mc-board-spend-provider"
    >
      <tr role="row" data-testid="mc-board-spend-group" className="mc-board-spend-row mc-board-spend-group">
        <th scope="rowgroup" role="rowheader" className="mc-board-spend-provider-name">
          {group.label}
        </th>
        <td role="cell" colSpan={SPEND_COLUMN_COUNT - 1} className="mc-board-spend-allowance">
          <div
            data-testid="mc-board-spend-window-grid"
            className="mc-board-spend-window-grid"
            style={{ gridTemplateColumns: BOARD_SPEND_WINDOW_COLUMNS[layout] }}
          >
            {group.windows.map((gauge) => (
              <SpendWindowGauge key={gauge.key} gauge={gauge} now={now} />
            ))}
          </div>
        </td>
      </tr>
      {group.models.length === 0 ? (
        <tr role="row" className="mc-board-spend-row">
          <td
            role="cell"
            colSpan={SPEND_COLUMN_COUNT}
            data-testid="mc-board-spend-provider-empty"
            className="mc-board-spend-empty"
          >
            no data
          </td>
        </tr>
      ) : (
        group.models.map((model) => (
          <tr
            key={model.key}
            role="row"
            data-testid="mc-board-spend-model"
            data-model-key={model.key}
            data-unassigned={model.unassigned ? "true" : "false"}
            className="mc-board-spend-row mc-board-spend-model"
          >
            <th scope="row" role="rowheader" className="mc-board-spend-model-name">
              {model.label}
            </th>
            <td role="cell" className="mc-board-spend-num mc-board-spend-model-share">
              {formatSharePct(model.sharePct)}
            </td>
            {model.figures.map((figure) => (
              <td
                key={figure.key}
                role="cell"
                data-figure={figure.key}
                className="mc-board-spend-num mc-board-spend-figure"
              >
                <AnimatedNumber
                  value={figure.value}
                  format={figure.key === "turns" ? formatCount : formatCompact}
                />
              </td>
            ))}
          </tr>
        ))
      )}
    </tbody>
  );
}

function SpendWindowGauge({ gauge, now }: { gauge: SpendWindow; now: number }) {
  const pct = useAnimatedNumber(gauge.pct);
  return (
    <div data-testid="mc-board-spend-gauge" data-window-key={gauge.key} className="mc-board-spend-gauge">
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
