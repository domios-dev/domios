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

export function supporterTokenTotals(label: string, value: SupporterSpendTotals) {
  return `${label}: ${value.input_tokens.toLocaleString()} in · ${value.output_tokens.toLocaleString()} out tokens`;
}

export function SupporterSpendLine({ spend }: { spend: SupporterSpend }) {
  return (
    <div role="status" aria-label="Supporter spend" className="flex flex-col gap-0.5">
      <span>
        <span className="text-[var(--ct-fg)]">{usd(spend.today)}</span> today ·{" "}
        <span className="text-[var(--ct-fg)]">{usd(spend.month)}</span> this month
        {spend.remaining_usd !== null && <> · estimated remaining ${spend.remaining_usd.toFixed(4)}</>}
      </span>
      {spend.exhausted && (
        <span role="alert" className="text-[var(--ct-yellow,#d97706)]">
          Balance exhausted. Supporter uses deterministic results until{" "}
          {spend.next_probe_at === null
            ? "the next check"
            : `the next check at ${new Date(spend.next_probe_at * 1000).toLocaleTimeString()}`}
          .
        </span>
      )}
      {!spend.storage_ready && <span role="alert">Supporter spend storage needs attention.</span>}
    </div>
  );
}

export function SupporterSpendSummary({ spend }: { spend: SupporterSpend }) {
  return (
    <span aria-label="Supporter cost">
      {usd(spend.today)} today · {usd(spend.month)} month
      {spend.remaining_usd !== null && <> · ${spend.remaining_usd.toFixed(4)} left</>}
    </span>
  );
}
