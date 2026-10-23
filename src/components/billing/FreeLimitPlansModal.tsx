import { useEffect, useId, useRef, useState } from "react";
import { Check } from "lucide-react";
import { publishNotice } from "../../lib/notices";
import { useAccount } from "../Settings/useAccount";
import { useAccountPlan } from "../Settings/useAccountPlan";
import { Button } from "../shared/Button";
import { FormDialog } from "../shared/FormDialog";
import { PlanCards } from "./PlanCards";
import { freeLimitGate, upgradePlan, UPGRADE_PLAN_KEY, useFreeLimitPrompt } from "./freeLimitGate";
import { limitHeadline, meshMachinesDetail, type FreeLimitKey } from "./freeLimitModel";
import { offerIdForPlan, upgradePlanKey, type BillingInterval, type PlanCatalog } from "./planCatalog";
import { usePlanCatalog } from "./planCatalogContext";
import { displayedPlan, subscribedPlan } from "./purchaseIntent";
import { billingFailureToast } from "../Settings/accountFailureToast";
import { usePurchaseIntent } from "./usePurchaseIntent";

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
  const account = useAccount();
  const identity = account.model.kind === "signed_in" ? account.model.identity : null;
  const signedIn = identity !== null;
  const billing = useAccountPlan(identity?.uid ?? null);
  const contextCatalog = usePlanCatalog();
  const offers = catalog ?? contextCatalog;
  const [refusedAsSubscribed, setRefusedAsSubscribed] = useState(false);
  const purchase = usePurchaseIntent({
    account,
    billing: { plan: billing.plan, planLoad: billing.planLoad, upgrade: (planKey) => onUpgrade(planKey).then(() => null, (failure: unknown) => failure ?? new Error("Checkout did not start")) },
    catalog: offers,
    notify: publishNotice,
    onCheckoutOpened: onClose,
    onAlreadySubscribed: () => setRefusedAsSubscribed(true),
  });
  const { intent } = purchase;
  const blocked = purchase.busy || (signedIn && billing.planLoad === "loading");
  const [interval, setInterval] = useState<BillingInterval>("year");
  const subscribed = subscribedPlan(billing.plan, refusedAsSubscribed);
  const manage = async () => {
    const failure = await billing.manage(() => {});
    if (failure) publishNotice(billingFailureToast(failure, "managing your subscription", billing.plan?.status, () => manage()));
  };
  const done = useRef<HTMLButtonElement>(null);
  useEffect(() => { if (intent.kind === "owned") done.current?.focus(); }, [intent.kind]);
  if (intent.kind === "owned") {
    const offerId = offerIdForPlan(offers, intent.planKey);
    const name = offers.offers.find((offer) => offer.id === offerId)?.name ?? offerId;
    return (
      <FormDialog titleId={titleId} descriptionId={descriptionId} className="max-w-3xl" onCancel={onClose}
        onSubmit={(event) => { event.preventDefault(); onClose(); }}>
        <span className="grid h-[34px] w-[34px] place-items-center rounded-full bg-[color-mix(in_srgb,var(--ct-green)_13%,transparent)] text-[var(--ct-green)]">
          <Check size={18} aria-hidden="true" />
        </span>
        <h2 id={titleId} className="m-0 text-base font-semibold">You already have {name}.</h2>
        <p id={descriptionId} className="m-0 text-xs text-[var(--ct-muted-fg)]">
          This limit doesn’t apply to your account. Carry on where you were.
        </p>
        <div className="flex justify-end">
          <Button ref={done} size="sm" variant="primary" type="submit">Done</Button>
        </div>
      </FormDialog>
    );
  }
  return (
    <FormDialog titleId={titleId} descriptionId={detail ? descriptionId : undefined} className="max-w-3xl" onCancel={onClose}
      onSubmit={(event) => { event.preventDefault(); if (!blocked && !subscribed) purchase.choose(upgradePlanKey(offers, interval) ?? UPGRADE_PLAN_KEY); }}>
      <h2 id={titleId} className="m-0 text-base font-semibold">{limitHeadline(limit, max)}</h2>
      {detail && <p id={descriptionId} className="m-0 text-xs text-[var(--ct-muted-fg)]">{detail}</p>}
      <PlanCards viewRequested catalog={offers} currentPlan={displayedPlan(billing.plan)} canPurchase={!blocked}
        interval={interval} onIntervalChange={setInterval}
        subscribedPlan={subscribed} onManage={() => void manage()} manageBusy={billing.pending}
        onChoose={purchase.choose} intent={intent} onCancel={purchase.cancel}
        signedInAs={identity ? identity.email ?? identity.display_name : null} />
      <div className="flex justify-end gap-2">
        <Button size="sm" onClick={onClose}>Not now</Button>
        {!subscribed && <Button size="sm" variant="primary" type="submit" loading={intent.kind === "opening"} disabled={blocked}>Upgrade</Button>}
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
