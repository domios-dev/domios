import { useId, useState } from "react";
import { Button } from "../shared/Button";
import { FormDialog } from "../shared/FormDialog";
import { PlanCards } from "./PlanCards";
import { freeLimitGate, upgradePlan, UPGRADE_PLAN_KEY, useFreeLimitPrompt } from "./freeLimitGate";
import { limitHeadline, type FreeLimitKey } from "./freeLimitModel";
import type { PlanCatalog } from "./planCatalog";

function PlansModal({ limit, max, onClose, onUpgrade, catalog }: {
  limit: FreeLimitKey;
  max?: number | null;
  onClose: () => void;
  onUpgrade: (planKey: string) => Promise<void>;
  catalog?: PlanCatalog;
}) {
  const titleId = useId();
  const descriptionId = useId();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const choose = (planKey: string) => {
    setBusy(true);
    setError(null);
    onUpgrade(planKey).then(onClose, (failure: unknown) => {
      setError(failure instanceof Error && failure.message ? failure.message : "Billing is unavailable right now.");
      setBusy(false);
    });
  };
  return (
    <FormDialog titleId={titleId} descriptionId={descriptionId} className="max-w-3xl" onCancel={onClose}
      onSubmit={(event) => { event.preventDefault(); choose(UPGRADE_PLAN_KEY); }}>
      <h2 id={titleId} className="m-0 text-base font-semibold">{limitHeadline(limit, max)}</h2>
      <p id={descriptionId} className="m-0 text-xs text-[var(--ct-muted-fg)]">
        Upgrade to remove this limit. Everything you already have stays as it is.
      </p>
      <PlanCards catalog={catalog} canPurchase={!busy} initialInterval="month" onChoose={choose} />
      {error && <p role="alert" className="m-0 text-xs text-[var(--ct-red)]">{error}</p>}
      <div className="flex justify-end gap-2">
        <Button size="sm" onClick={onClose}>Not now</Button>
        <Button size="sm" variant="primary" type="submit" loading={busy}>Upgrade</Button>
      </div>
    </FormDialog>
  );
}

/** Mounted once; any creation trigger opens it through `freeLimitGate`. */
export function FreeLimitPlansModal({ upgrade = upgradePlan }: { upgrade?: (planKey: string) => Promise<void> }) {
  const limit = useFreeLimitPrompt();
  if (!limit) return null;
  return <PlansModal key={limit} limit={limit} max={freeLimitGate.limits()?.max[limit]} onClose={freeLimitGate.close} onUpgrade={upgrade} />;
}
