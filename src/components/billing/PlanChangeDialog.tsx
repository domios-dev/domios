import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useId } from "react";
import { Button } from "../shared/Button";
import { FormDialog } from "../shared/FormDialog";
import { tierName } from "./desktopBilling";

export interface SelectedPlanChange {
  priceId: string;
  tier: string;
  direction: "upgrade" | "downgrade";
}

export function PlanChangeDialog({ selection, currentTier, periodEnd, pending, error, submitted, onSubmit, onClose }: {
  selection: SelectedPlanChange; currentTier: string; periodEnd: number | null; pending: boolean;
  error: string | null; submitted: boolean; onSubmit: () => void; onClose: () => void;
}) {
  const title = useId();
  const date = periodEnd ? new Date(periodEnd * 1000).toLocaleDateString() : "your next renewal";
  return <FormDialog titleId={title} onCancel={onClose} onSubmit={event => { event.preventDefault(); if (!pending && !submitted) onSubmit(); }}>
    <h2 id={title}>{selection.direction === "upgrade" ? "Upgrade" : "Downgrade"} to {tierName(selection.tier)}</h2>
    <p>{selection.direction === "upgrade" ? "Your upgrade takes effect after payment confirmation. Paddle prorates the charge for the remaining billing period."
      : `${tierName(currentTier)} until ${date}, ${tierName(selection.tier)} from ${date}.`}</p>
    <p>Plan changes keep the same billing interval. Final charges are determined by Paddle.</p>
    {error && <InlineErrorNotice error={error} operation="changing your plan" role="status" />}
    <Button type="submit" disabled={pending || submitted}>Confirm plan change</Button>
    <Button type="button" onClick={onClose}>Cancel</Button>
  </FormDialog>;
}
