import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useId } from "react";
import { Button } from "../shared/Button";
import { FormDialog } from "../shared/FormDialog";
import { planChangeCopy, tierName, type PlanChangeMode } from "./desktopBilling";

export interface SelectedPlanChange {
  priceId: string;
  tier: string;
  direction: "upgrade" | "downgrade";
}

export function PlanChangeDialog({ selection, currentTier, periodEnd, planChangeMode = null, pending, error, submitted, onSubmit, onClose }: {
  selection: SelectedPlanChange; currentTier: string; periodEnd: number | null; pending: boolean;
  planChangeMode?: PlanChangeMode | null;
  error: string | null; submitted: boolean; onSubmit: () => void; onClose: () => void;
}) {
  const title = useId();
  const copy = planChangeCopy(planChangeMode, selection.direction);
  const date = periodEnd ? new Date(periodEnd * 1000).toLocaleDateString() : null;
  const timing = copy.dialog(currentTier, selection.tier, date);
  return <FormDialog titleId={title} onCancel={onClose} onSubmit={event => { event.preventDefault(); if (!pending && !submitted) onSubmit(); }}>
    <h2 id={title}>{selection.direction === "upgrade" ? "Upgrade" : "Downgrade"} to {tierName(selection.tier)}</h2>
    {timing && <p>{timing}</p>}
    <p>{copy.terms}</p>
    {error && <InlineErrorNotice error={error} operation="changing your plan" role="status" />}
    <Button type="submit" disabled={pending || submitted}>Confirm plan change</Button>
    <Button type="button" onClick={onClose}>Cancel</Button>
  </FormDialog>;
}
