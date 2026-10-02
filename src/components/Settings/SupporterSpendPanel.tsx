import { useEffect, useState, type FormEvent } from "react";
import { Button } from "../shared/Button";
import { FormRow } from "../shared/FormRow";
import type { SupporterSpend, SupporterSpendTotals } from "./useDecisionToken";

interface Props {
  adapter: string;
  spend: SupporterSpend;
  busy: boolean;
  setBalance: (balance: number | null) => Promise<void>;
}

function totals(label: string, value: SupporterSpendTotals) {
  return `${label}: ${value.input_tokens.toLocaleString()} input tokens · ${value.output_tokens.toLocaleString()} output tokens · ${value.estimated_usd === null ? "Price not reported" : `$${value.estimated_usd.toFixed(6)} estimated`}`;
}

export function SupporterSpendPanel({ adapter, spend, busy, setBalance }: Props) {
  const [balance, changeBalance] = useState(spend.balance_usd?.toString() ?? "");
  useEffect(() => {
    changeBalance(spend.balance_usd?.toString() ?? "");
  }, [adapter, spend.balance_usd]);

  const save = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const value = balance.trim() === "" ? null : Number(balance);
    if (value !== null && (!Number.isFinite(value) || value < 0)) return;
    await setBalance(value);
  };

  return (
    <div className="flex flex-col gap-2 text-xs text-[var(--ct-muted-fg)]">
      <div role="status" aria-label="Supporter spend" className="flex flex-col gap-1">
        <span>{totals("Today", spend.today)}</span>
        <span>{totals("This month", spend.month)}</span>
        {spend.remaining_usd !== null && <span>Remaining: ${spend.remaining_usd.toFixed(4)} (estimate)</span>}
        {spend.exhausted && (
          <span role="alert">
            Balance exhausted. Supporter uses deterministic results. The next normal request can check again after{" "}
            {spend.next_probe_at === null
              ? "a new balance is entered"
              : new Date(spend.next_probe_at * 1000).toLocaleString()}
            .
          </span>
        )}
        {!spend.storage_ready && <span role="alert">Supporter spend storage needs attention.</span>}
      </div>
      <form onSubmit={(event) => void save(event)} className="flex items-end gap-2">
        <FormRow
          label="Balance (USD)"
          htmlFor="supporter-balance"
          hint="Optional. Enter the current balance after a top-up; remaining is estimated from subsequent usage."
        >
          <input
            id="supporter-balance"
            aria-label="Balance (USD)"
            type="number"
            min="0"
            step="any"
            value={balance}
            onChange={(event) => changeBalance(event.target.value)}
            disabled={busy}
            className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)]"
          />
        </FormRow>
        <Button type="submit" variant="ghost" size="sm" disabled={busy}>
          Save balance
        </Button>
      </form>
    </div>
  );
}
