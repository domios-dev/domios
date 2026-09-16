import { Fragment, useId, useState } from "react";
import type { AnalyticsGroupBy, AnalyticsSnapshot } from "@codeterm/plugin-sdk";
import { AnimatedNumber } from "../../../../../src/components/Canvas/missionControl/AnimatedNumber";
import { BoardPanel, BoardPanelSection, Facts, ShowAll } from "../../../../../src/components/Canvas/missionControl/BoardPanel";
import { spendChartToneAttr, type SpendChartTone } from "../../../../../src/components/Canvas/missionControl/spendChart";
import { Segmented } from "../../../../../src/components/shared/Segmented";
import { breakdown, BREAKDOWN_PREVIEW, type BreakdownRow } from "../model/breakdown";
import { formatCompact, formatCount, formatSharePct } from "../model/format";
import { GROUP_BY_OPTIONS } from "../model/query";

const HEADINGS: Record<AnalyticsGroupBy, string> = {
  provider_model: "Provider and model",
  workspace: "Workspace",
  team: "Team",
  plan: "Plan",
};

const UNNAMED_COPY: Record<AnalyticsGroupBy, string> = {
  provider_model: "didn't report a model",
  workspace: "were recorded without a workspace",
  team: "belong to no team",
  plan: "ran outside any plan",
};

export function Breakdown({
  snapshot,
  tones,
  groupBy,
  onGroupBy,
  narrow,
}: {
  snapshot: AnalyticsSnapshot;
  tones: ReadonlyMap<string, SpendChartTone>;
  groupBy: AnalyticsGroupBy;
  onGroupBy: (groupBy: AnalyticsGroupBy) => void;
  narrow: boolean;
}) {
  const { rows, totalTurns, unnamedTurns } = breakdown(snapshot, tones);
  const [showAll, setShowAll] = useState(false);
  const [expanded, setExpanded] = useState<string[]>([]);
  const tableId = useId();
  const visible = showAll ? rows : rows.slice(0, BREAKDOWN_PREVIEW);
  const heading = HEADINGS[groupBy];

  const renderRow = (row: BreakdownRow, depth: 0 | 1, index: string) => {
    const open = expanded.includes(row.id);
    const detailId = `${tableId}-detail-${index}`;
    return (
      <Fragment key={row.id}>
        <tr
          data-testid="analytics-breakdown-row"
          data-depth={depth}
          data-unassigned={row.unnamed ? "true" : "false"}
          className="mc-board-models-row"
        >
          <th scope="row" className="mc-board-models-name">
            {row.tone !== null && <span aria-hidden="true" className="mc-board-swatch" data-tone={spendChartToneAttr(row.tone)} />}
            <span className={depth === 1 ? "analytics-child-name" : undefined}>{row.label}</span>
            {narrow && (
              <button
                type="button"
                className="mc-board-more analytics-row-toggle"
                aria-expanded={open}
                aria-controls={open ? detailId : undefined}
                aria-label={`${open ? "Hide" : "Show"} figures for ${row.label}`}
                onClick={() => setExpanded((ids) => (open ? ids.filter((id) => id !== row.id) : [...ids, row.id]))}
              >
                {open ? "Less" : "More"}
              </button>
            )}
          </th>
          <td className="mc-board-models-num">
            <AnimatedNumber value={row.totals.new_input} format={formatCompact} />
          </td>
          <td className="mc-board-models-num">{formatSharePct(row.sharePct)}</td>
          {!narrow && (
            <>
              <td className="mc-board-models-num">{formatCompact(row.totals.generated)}</td>
              <td className="mc-board-models-num">{formatCompact(row.totals.read_from_cache)}</td>
              <td className="mc-board-models-num">{formatCount(row.totals.turns)}</td>
            </>
          )}
        </tr>
        {narrow && open && (
          <tr id={detailId} className="analytics-breakdown-detail">
            <td colSpan={3}>
              <Facts
                items={[
                  { key: "generated", node: `Generated ${formatCompact(row.totals.generated)}` },
                  { key: "cache", node: `Cache ${formatCompact(row.totals.read_from_cache)}` },
                  { key: "turns", node: `Turns ${formatCount(row.totals.turns)}` },
                ]}
              />
            </td>
          </tr>
        )}
      </Fragment>
    );
  };

  return (
    <BoardPanel
      testId="analytics-breakdown"
      title="Breakdown"
      meta={
        <Segmented
          value={groupBy}
          options={GROUP_BY_OPTIONS}
          onChange={onGroupBy}
          aria-label="Group by"
          data-testid="analytics-group-by"
          className="analytics-seg"
          height={narrow ? 36 : 24}
        />
      }
    >
      <BoardPanelSection>
        {rows.length === 0 ? (
          <p data-testid="analytics-breakdown-empty" className="mc-board-note">
            No turns recorded in this range.
          </p>
        ) : (
          <table id={tableId} className="mc-board-models-table analytics-breakdown">
            <caption className="analytics-sr">New input by {heading.toLowerCase()}</caption>
            <thead>
              <tr>
                <th scope="col" className="mc-board-models-name">
                  {heading}
                </th>
                <th scope="col" className="mc-board-models-num">
                  new input
                </th>
                <th scope="col" className="mc-board-models-num">
                  share
                </th>
                {!narrow && (
                  <>
                    <th scope="col" className="mc-board-models-num">
                      generated
                    </th>
                    <th scope="col" className="mc-board-models-num">
                      cache
                    </th>
                    <th scope="col" className="mc-board-models-num">
                      turns
                    </th>
                  </>
                )}
              </tr>
            </thead>
            <tbody>
              {visible.map((parent, index) => (
                <Fragment key={parent.id}>
                  {renderRow(parent, 0, String(index))}
                  {parent.children.map((child, childIndex) => renderRow(child, 1, `${index}-${childIndex}`))}
                </Fragment>
              ))}
            </tbody>
          </table>
        )}
        <ShowAll
          total={rows.length}
          limit={BREAKDOWN_PREVIEW}
          open={showAll}
          controls={tableId}
          testId="analytics-breakdown-more"
          onToggle={() => setShowAll((value) => !value)}
        />
        <p className="mc-board-caption">
          {unnamedTurns > 0 &&
            `${formatCount(unnamedTurns)} of ${formatCount(totalTurns)} turns ${UNNAMED_COPY[groupBy]}; they keep their own row and are never assigned elsewhere. `}
          Share is of all new input in range.
        </p>
      </BoardPanelSection>
    </BoardPanel>
  );
}
