import { useId, useState } from "react";
import { AnimatedNumber } from "./AnimatedNumber";
import { BoardPanel, BoardPanelSection, ShowAll } from "./BoardPanel";
import {
  formatCompact,
  formatSharePct,
  MODELS_PREVIEW,
  modelSpendRows,
  SPEND_SCOPE_CAPTION,
  spendReading,
} from "./missionControlBoardModel";
import type { MissionControlSpend } from "./missionControlState";
import { TEAM_SCOPE_CAPTION, teamModelRows, teamSpendNote, type TeamSpend } from "./teamSpend";

/** Models ranked by new input, the spend that costs; the long tail waits behind Show all. */
export function ModelsCard({ spend, teamSpend = null }: { spend: MissionControlSpend; teamSpend?: TeamSpend | null }) {
  const rows = teamSpend ? teamModelRows(teamSpend) : modelSpendRows(spend);
  const [showAll, setShowAll] = useState(false);
  const tableId = useId();
  const visible = showAll ? rows : rows.slice(0, MODELS_PREVIEW);
  return (
    <BoardPanel testId="mc-board-models" title="Models" meta={teamSpend ? TEAM_SCOPE_CAPTION : SPEND_SCOPE_CAPTION}>
      <BoardPanelSection>
        {rows.length === 0 ? (
          <p data-testid="mc-board-models-empty" className="mc-board-note">
            {teamSpend ? teamSpendNote(teamSpend) ?? "no data" : spendReading(spend).note ?? "no data"}
          </p>
        ) : (
          <>
            <table id={tableId} className="mc-board-models-table">
              <thead>
                <tr>
                  <th scope="col" className="mc-board-models-name">
                    model
                  </th>
                  <th scope="col" className="mc-board-models-num">
                    new input
                  </th>
                  <th scope="col" className="mc-board-models-num">
                    share
                  </th>
                </tr>
              </thead>
              <tbody>
                {visible.map((row) => (
                  <tr
                    key={row.key}
                    data-testid="mc-board-spend-model"
                    data-model-key={row.key}
                    data-unassigned={row.unassigned ? "true" : "false"}
                    className="mc-board-models-row"
                  >
                    <th scope="row" className="mc-board-models-name">
                      <span aria-hidden="true" className="mc-board-swatch" data-tone={row.tone} />
                      {row.label}
                    </th>
                    <td className="mc-board-models-num">
                      <AnimatedNumber value={row.newInput} format={formatCompact} />
                    </td>
                    <td className="mc-board-models-num">{formatSharePct(row.sharePct)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <ShowAll
              total={rows.length}
              limit={MODELS_PREVIEW}
              open={showAll}
              controls={tableId}
              testId="mc-board-models-more"
              onToggle={() => setShowAll((value) => !value)}
            />
          </>
        )}
      </BoardPanelSection>
    </BoardPanel>
  );
}
