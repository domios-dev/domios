import { useId, useState } from "react";
import { Button } from "../shared/Button";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { FormDialog } from "../shared/FormDialog";
import { PlanCards } from "./PlanCards";
import { freeLimitGate, upgradePlan, UPGRADE_PLAN_KEY, useFreeLimitPrompt } from "./freeLimitGate";
import { limitHeadline, meshMachinesDetail, type FreeLimitKey } from "./freeLimitModel";
import type { PlanCatalog } from "./planCatalog";

function PlansModal({ limit, max, detail, onClose, onUpgrade, catalog }: {
  limit: FreeLimitKey;
  max?: number | null;
  detail?: string;
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
        {detail && <>{detail} </>}Upgrade to remove this limit. Everything you already have stays as it is.
      </p>
      <PlanCards catalog={catalog} canPurchase={!busy} initialInterval="month" onChoose={choose} />
      {error && <InlineErrorNotice description={{ title: "Checkout did not start", cause: error, actions: [], raw: "" }} role="alert" />}
      <div className="flex justify-end gap-2">
        <Button size="sm" onClick={onClose}>Not now</Button>
        <Button size="sm" variant="primary" type="submit" loading={busy}>Upgrade</Button>
      </div>
    </FormDialog>
  );
}

/** Mounted once; any creation trigger opens it through `freeLimitGate`. */
export function FreeLimitPlansModal({ upgrade = upgradePlan }: { upgrade?: (planKey: string) => Promise<void> }) {
  const prompt = useFreeLimitPrompt();
  if (!prompt) return null;
  if (prompt.key === "mesh.machines") {
    return <PlansModal key={`mesh:${prompt.requestId}`} limit={prompt.key} max={prompt.limit} detail={meshMachinesDetail(prompt)}
      onClose={freeLimitGate.close} onUpgrade={upgrade} />;
  }
  return <PlansModal key={prompt.key} limit={prompt.key} max={freeLimitGate.limits()?.max[prompt.key]} onClose={freeLimitGate.close} onUpgrade={upgrade} />;
}
