import type { ApiConfig } from "../../lib/types";
import type { SupporterStatus } from "../Settings/useDecisionToken";
import { useSupporterResource } from "../Supporter/useSupporterResource";

function estimate(amount: number | null): string {
  if (amount === null || !Number.isFinite(amount)) return "—";
  return amount >= 1 ? `$${amount.toFixed(2)}` : `$${amount.toFixed(4)}`;
}

/** The Spinal Cord's spend and request pace as compact stat tiles. */
export function SupporterAllowanceRow({ api }: { api?: ApiConfig }) {
  const { data: status } = useSupporterResource<SupporterStatus>(api, "/api/supporter/status");
  if (!status?.connected) return null;
  const pricing = status.pricing;
  const priced = pricing && (pricing.input_usd_per_million > 0 || pricing.output_usd_per_million > 0);
  if (!priced && status.calls_last_day === 0 && status.calls_last_hour === 0) return null;
  const cap = status.budget.calls_per_hour;
  return (
    <div
      role="status"
      aria-label="Spinal Cord allowance"
      data-cap={cap || "none"}
      data-requests-hour={status.calls_last_hour}
      data-requests-day={status.calls_last_day}
      className="ct-spend"
    >
      {pricing && priced && (
        <>
          <span className="ct-spend__tile" data-estimate="today" data-usd={status.spend.today.estimated_usd ?? "unknown"}>
            <span className="ct-spend__value">{estimate(status.spend.today.estimated_usd)}</span>
            <span className="ct-spend__label">today</span>
          </span>
          <span className="ct-spend__tile" data-estimate="month" data-usd={status.spend.month.estimated_usd ?? "unknown"}>
            <span className="ct-spend__value">{estimate(status.spend.month.estimated_usd)}</span>
            <span className="ct-spend__label">this month</span>
          </span>
        </>
      )}
      <span className="ct-spend__tile">
        <span className="ct-spend__value">{status.calls_last_hour}{cap ? <span className="ct-spend__cap">/{cap}</span> : null}</span>
        <span className="ct-spend__label">per hour</span>
      </span>
    </div>
  );
}
