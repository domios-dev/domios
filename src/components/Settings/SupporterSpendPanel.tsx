import { formatCompact, formatExact } from "../../lib/formatCompact";
import type { SupporterSpend, SupporterSpendTotals, SupporterStatus } from "./useDecisionToken";

export function supporterIsPriced(price: SupporterStatus["pricing"]): boolean {
  return Boolean(
    price &&
      Number.isFinite(price.input_usd_per_million) &&
      Number.isFinite(price.output_usd_per_million) &&
      price.input_usd_per_million >= 0 &&
      price.output_usd_per_million >= 0 &&
      (price.input_usd_per_million > 0 || price.output_usd_per_million > 0),
  );
}

function usd(value: SupporterSpendTotals) {
  return value.estimated_usd === null || !Number.isFinite(value.estimated_usd)
    ? "price not reported"
    : `$${value.estimated_usd.toFixed(4)}`;
}

export function supporterActivity(callsLastHour: number, callsLastDay: number): { text: string; title: string } {
  return {
    text: `${formatCompact(callsLastHour)} req/hr · ${formatCompact(callsLastDay)} req/day`,
    title: `${formatExact(callsLastHour)} requests in the last hour, ${formatExact(callsLastDay)} in the last day`,
  };
}

export function SupporterSpendLine({ spend }: { spend: SupporterSpend }) {
  return (
    <div role="status" aria-label="Spinal Cord spend" className="flex flex-col gap-0.5">
      <span>
        <span className="text-[var(--ct-fg)]">{usd(spend.today)}</span> today ·{" "}
        <span className="text-[var(--ct-fg)]">{usd(spend.month)}</span> this month
      </span>
      {spend.exhausted && (
        <span role="alert" className="text-[var(--ct-yellow,#d97706)]">
          Credit unavailable. Supporter uses deterministic results until{" "}
          {spend.next_probe_at === null
            ? "the next check"
            : `the next check at ${new Date(spend.next_probe_at * 1000).toLocaleTimeString()}`}
          .
        </span>
      )}
      {!spend.storage_ready && <span role="alert">Spinal Cord spend storage needs attention.</span>}
    </div>
  );
}

export function SupporterSpendSummary({ spend }: { spend: SupporterSpend }) {
  return (
    <span aria-label="Spinal Cord cost">
      {usd(spend.today)} today · {usd(spend.month)} month
    </span>
  );
}
