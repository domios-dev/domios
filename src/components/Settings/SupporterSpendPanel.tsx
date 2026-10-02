import { useEffect, useState, type FormEvent } from "react";
import { Button } from "../shared/Button";
import { UI_INPUT } from "../shared/design";
import type { SupporterSpend, SupporterSpendTotals } from "./useDecisionToken";

function usd(value: SupporterSpendTotals) {
  return value.estimated_usd === null ? "price not reported" : `$${value.estimated_usd.toFixed(4)}`;
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
        {spend.remaining_usd !== null && <> · ${spend.remaining_usd.toFixed(4)} left (estimate)</>}
      </span>
      {spend.exhausted && (
        <span role="alert" className="text-[var(--ct-yellow,#d97706)]">
          Balance exhausted. Supporter uses deterministic results until{" "}
          {spend.next_probe_at === null
            ? "a new balance is entered"
            : `the next check at ${new Date(spend.next_probe_at * 1000).toLocaleTimeString()}`}
          .
        </span>
      )}
      {!spend.storage_ready && <span role="alert">Supporter spend storage needs attention.</span>}
    </div>
  );
}

interface BalanceProps {
  adapter: string;
  spend: SupporterSpend;
  busy: boolean;
  setBalance: (balance: number | null) => Promise<void>;
}

export function SupporterBalanceForm({ adapter, spend, busy, setBalance }: BalanceProps) {
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
    <form onSubmit={(event) => void save(event)} className="flex flex-wrap items-center gap-2">
      <label htmlFor="supporter-balance">Balance after top-up, USD</label>
      <input
        id="supporter-balance"
        aria-label="Balance (USD)"
        type="number"
        min="0"
        step="any"
        value={balance}
        onChange={(event) => changeBalance(event.target.value)}
        disabled={busy}
        placeholder="Optional"
        className={`${UI_INPUT.compact} w-28`}
      />
      <Button type="submit" variant="ghost" size="sm" disabled={busy}>
        Save balance
      </Button>
    </form>
  );
}
