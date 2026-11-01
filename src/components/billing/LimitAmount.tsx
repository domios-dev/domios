import { Infinity as InfinityIcon } from "lucide-react";

/** A limit's number, or an infinity sign in the text colour that screen readers announce as Unlimited. */
export function LimitAmount({ amount }: { amount: number | null }) {
  if (amount !== null) return <span className="tabular-nums">{amount}</span>;
  // A line-tall box centres the sign on the text line instead of sitting it on the baseline.
  return (
    <span className="inline-flex h-lh items-center align-top">
      <InfinityIcon size={14} strokeWidth={2.25} aria-hidden="true" />
      <span className="sr-only">Unlimited</span>
    </span>
  );
}
