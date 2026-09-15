import type { AnalyticsSnapshot } from "@codeterm/plugin-sdk";
import { BoardPanel, BoardPanelSection } from "../../../../../src/components/Canvas/missionControl/BoardPanel";
import { allowanceAccounts, filtersBeyondProvider, silentProviders } from "../model/allowance";
import { formatClock, formatDay, formatSharePct } from "../model/format";
import { providerLabels, providerOptions, type Catalog } from "../model/options";
import type { FilterState } from "../model/query";

export function AllowanceCard({ snapshot, catalog, filters }: { snapshot: AnalyticsSnapshot; catalog: Catalog; filters: FilterState }) {
  const labels = providerLabels(catalog);
  const accounts = allowanceAccounts(snapshot.allowance, labels);
  const inScope = providerOptions(catalog)
    .map((option) => option.value)
    .filter((id) => filters.providers.length === 0 || filters.providers.includes(id));
  const silent = silentProviders(inScope, snapshot.allowance, labels);
  const readAt = accounts.reduce<number | null>((latest, account) => Math.max(latest ?? 0, account.capturedAt), null);

  return (
    <BoardPanel
      testId="analytics-allowance"
      title="Allowance"
      meta={readAt === null ? "per provider account" : `per provider account · read ${formatClock(readAt)}`}
    >
      <BoardPanelSection>
        {accounts.length === 0 && silent.length === 0 && (
          <p className="mc-board-note">No provider on this machine has reported allowance.</p>
        )}
        {accounts.some((account) => account.windows.length > 0) && (
          <ul className="analytics-meters" role="list">
            {accounts.flatMap((account) =>
              account.windows.map((window) => {
                const labelId = `analytics-meter-${window.key}`;
                const clamped = Math.max(0, Math.min(100, window.pct));
                const resets = window.resetsAt === null ? null : `resets ${formatDay(window.resetsAt)} ${formatClock(window.resetsAt)}`;
                const detail = [resets, account.staleReason ? "may be out of date" : null].filter(Boolean).join(" · ");
                return (
                  <li key={window.key} data-testid="analytics-allowance-window" className="analytics-meter" data-state={window.state}>
                    <span id={labelId} className="analytics-meter-label">
                      {window.label}
                    </span>
                    <span className="analytics-meter-pct">{formatSharePct(window.pct)}</span>
                    <span
                      role="meter"
                      aria-labelledby={labelId}
                      aria-valuemin={0}
                      aria-valuemax={100}
                      aria-valuenow={clamped}
                      className="analytics-meter-bar"
                    >
                      <span style={{ width: `${clamped}%` }} />
                    </span>
                    {detail && <span className="analytics-meter-reset">{detail}</span>}
                  </li>
                );
              }),
            )}
          </ul>
        )}
        {accounts
          .filter((account) => account.windows.length === 0)
          .map((account) => (
            <p key={account.provider} className="mc-board-note">
              {account.label} reported no allowance figures.
            </p>
          ))}
        {silent.map((label) => (
          <p key={label} data-testid="analytics-allowance-silent" className="mc-board-note">
            {label} hasn't reported allowance.
          </p>
        ))}
        <p className="mc-board-caption">
          {filtersBeyondProvider(filters)
            ? "These are whole provider accounts. Allowance isn't reported per model, workspace, team or plan, so those filters don't narrow it."
            : "Providers report allowance for a whole account, never per model, workspace, team or plan."}
        </p>
      </BoardPanelSection>
    </BoardPanel>
  );
}
