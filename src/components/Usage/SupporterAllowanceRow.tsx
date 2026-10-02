import type { ApiConfig } from "../../lib/types";
import type { SupporterStatus } from "../Settings/useDecisionToken";
import { useSupporterResource } from "../Supporter/useSupporterResource";

function estimate(amount: number | null): string {
  return amount === null || !Number.isFinite(amount) ? "unavailable" : `$${amount.toFixed(4)}`;
}

export function SupporterAllowanceRow({ api }: { api?: ApiConfig }) {
  const { data: status } = useSupporterResource<SupporterStatus>(api, "/api/supporter/status");
  if (!status?.connected) return null;
  const pricing = status.pricing;
  const priced = pricing && (pricing.input_usd_per_million > 0 || pricing.output_usd_per_million > 0);
  if (!priced && status.calls_last_day === 0 && status.calls_last_hour === 0) return null;
  const remaining = status.spend.remaining_usd;
  return (
    <div
      role="status"
      aria-label="Supporter allowance"
      data-cap={status.budget.calls_per_hour || "none"}
      data-requests-hour={status.calls_last_hour}
      data-requests-day={status.calls_last_day}
      className="flex flex-wrap gap-x-3 gap-y-1 p-2 text-xs text-[var(--ct-muted-fg)]"
    >
      {pricing && priced && (
        <>
          <span data-estimate="today" data-usd={status.spend.today.estimated_usd ?? "unknown"}>
            {pricing.display_name} · today estimate: {estimate(status.spend.today.estimated_usd)}
          </span>
          <span data-estimate="month" data-usd={status.spend.month.estimated_usd ?? "unknown"}>
            This month estimate: {estimate(status.spend.month.estimated_usd)}
          </span>
          {remaining !== null && (
            <span data-estimate="remaining" data-usd={remaining}>
              Remaining balance estimate: {estimate(remaining)}
            </span>
          )}
          {remaining !== null && remaining < 1 && (
            <span role="alert" data-balance-state="below_one">Balance estimate below $1</span>
          )}
        </>
      )}
      <span>{status.calls_last_hour} Supporter requests per hour · {status.calls_last_day} per day</span>
      {status.budget.calls_per_hour ? <span>Cap: {status.budget.calls_per_hour} requests per hour</span> : null}
    </div>
  );
}
