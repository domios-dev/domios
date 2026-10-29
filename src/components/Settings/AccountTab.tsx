import { createNoticePublisher } from "../../lib/notices";
import { useEffect, useRef, useState } from "react";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { CreditCard, UserRound } from "lucide-react";
import { Button } from "../shared/Button";
import { PlanCards } from "../billing/PlanCards";
import { GiftRedemption } from "../billing/GiftRedemption";
import { useAccountUsage } from "../billing/useAccountUsage";
import { AccountLimits } from "../billing/AccountLimits";
import { useDesktopBilling } from "../billing/useDesktopBilling";
import { scheduledPlanCopy, tierName } from "../billing/desktopBilling";
import { PlanChangeDialog, type SelectedPlanChange } from "../billing/PlanChangeDialog";
import { usePlanCatalog } from "../billing/planCatalogContext";
import { usePurchaseIntent } from "../billing/usePurchaseIntent";
import { ACCOUNT_ENTRY_OPTIONS, SIGN_IN_PROVIDER_OPTIONS } from "./accountSignInOptions";
import { CopyRow, SettingsCard, SettingsPage, SettingsPill } from "./shared";
import { useAccountPlan } from "./useAccountPlan";
import { useAccount } from "./useAccount";
import { useAccountEnrollmentFailure } from "./useAccountEnrollmentFailure";
import { failureMessage } from "../MeshPanel/accountMachines";
import { SettingsCardAnchor } from "./SettingsCardAnchor";
import type { SettingsCardAnchorId } from "./sections";
import { useMultiplexer } from "../../store/multiplexer";
import { billingFailureToast } from "./accountFailureToast";
import { SESSION_REVOKED_MESSAGE, accountInitials, type LoginHint } from "./accountModel";

const MUTED = "m-0 text-xs text-[var(--ct-muted-fg)]";

export function AccountTab({ focusCard }: { focusCard?: SettingsCardAnchorId }) {
  const { upsertToast } = useMultiplexer();
  const notify = createNoticePublisher({ upsertToast });
  const { model, signIn, signOut } = useAccount((message) => notify({
    kind: "toast", id: "account-sign-out-failure", title: "Sign-out status", detail: message,
    severity: "Warning", tabId: null, workspaceId: null,
  }));
  const billing = useAccountPlan(model.kind === "signed_in" ? model.identity.uid : null);
  const fallbackCatalog = usePlanCatalog();
  const desktopBilling = useDesktopBilling(model.kind === "signed_in" ? model.identity.uid : null);
  const catalog = desktopBilling.catalog ?? fallbackCatalog;
  const [planChange, setPlanChange] = useState<(SelectedPlanChange & { identity: string }) | null>(null);
  const [plansRequest, setPlansRequest] = useState(0);
  const [refusedAsSubscribed, setRefusedAsSubscribed] = useState(false);
  const retryHint = useRef<LoginHint | undefined>(undefined);
  const signedIn = model.kind === "signed_in";
  const identity = signedIn ? model.identity.uid : null;
  const enrollmentFailure = useAccountEnrollmentFailure(identity);
  useEffect(() => setRefusedAsSubscribed(false), [identity]);
  const usage = useAccountUsage(identity);
  const offersReady = desktopBilling.catalog !== null && desktopBilling.subscription !== null;
  const paidSubscription = desktopBilling.subscription?.paid_subscription;
  const subscriptionPlan = paidSubscription === undefined ? undefined : paidSubscription?.tier ?? null;
  const purchase = usePurchaseIntent({
    account: { model, signIn },
    billing,
    catalog,
    subscriptionPlan,
    ready: !signedIn || offersReady,
    failed: signedIn && !desktopBilling.loading && !offersReady,
    notify,
    retrySignIn: () => { void signIn(retryHint.current); },
    onAlreadySubscribed: () => setRefusedAsSubscribed(true),
  });
  const revealPlans = () => setPlansRequest((count) => count + 1);
  const manageSubscription = async () => {
    if (!paidSubscription && !refusedAsSubscribed) { revealPlans(); return; }
    const failure = await billing.manage();
    if (failure) notify(billingFailureToast(failure, "managing your subscription", billing.plan?.status, () => manageSubscription()));
  };
  const staff = billing.plan?.fullAccess === true;
  const startSignIn = (hint: LoginHint) => {
    retryHint.current = hint;
    void signIn(hint);
  };
  return <SettingsPage title="Account">
    <SettingsCard icon={<UserRound size={16} />} title="Domios account">
      {model.kind === "signed_in" ? (() => {
        const name = model.identity.display_name ?? model.identity.email ?? model.identity.uid;
        return <div className="flex flex-col gap-3">
          <div className="flex items-center gap-3">
            <span aria-hidden className="grid h-10 w-10 shrink-0 place-items-center rounded-full bg-[color-mix(in_srgb,var(--ct-accent)_18%,transparent)] text-sm font-semibold text-[var(--ct-accent)]">
              {accountInitials(name)}
            </span>
            <div className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-sm font-medium text-[var(--ct-fg)]">{name}</span>
              {model.identity.email && model.identity.email !== name && <span className={`${MUTED} truncate`}>{model.identity.email}</span>}
            </div>
            <Button size="sm" variant="ghost" className="text-xs" onClick={() => void signOut()}>Sign out</Button>
          </div>
          {model.sessionOnly && <p role="status" className={MUTED}>Signed in for this session only: secure storage is unavailable, so sign in again after restarting Domios.</p>}
          <CopyRow label="Account ID" value={model.identity.uid} />
          {enrollmentFailure && <InlineErrorNotice description={{ title: "This machine could not join your account", cause: failureMessage(enrollmentFailure), actions: [], raw: "" }} />}
        </div>;
      })() : <div className="flex flex-col items-start gap-3 text-xs">
        {model.kind === "signed_out" && model.sessionRevoked && <p role="status" className={MUTED}>{SESSION_REVOKED_MESSAGE}</p>}
        {model.kind === "pending"
          ? <p role="status" className={MUTED}>Finish signing in in your browser.</p>
          : <p className={MUTED}>Sign in to keep your plan and settings with your account.</p>}
        {model.kind === "error" && !model.failure && <InlineErrorNotice error={model.message} operation="updating your account" blocking />}
        <div className="flex flex-wrap items-center gap-2">
          {ACCOUNT_ENTRY_OPTIONS.map((option, index) =>
            <Button key={option.label} size="sm" variant={index === 0 ? "primary" : "ghost"} disabled={model.kind === "pending"}
              onClick={() => startSignIn(option.hint)}>{option.label}</Button>)}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {SIGN_IN_PROVIDER_OPTIONS.map((option) =>
            <Button key={option.label} size="sm" variant="ghost" className="gap-2" disabled={model.kind === "pending"}
              onClick={() => startSignIn(option.hint)}>{option.icon}{option.label}</Button>)}
        </div>
      </div>}
    </SettingsCard>
    <SettingsCard icon={<CreditCard size={16} />} title="Plan">
      <div className="flex flex-col items-start gap-3">
        <div className="flex w-full items-center gap-2">
          <span className="text-xl font-semibold text-[var(--ct-fg)]">{tierName(billing.plan?.plan ?? (signedIn ? "unavailable" : "free"))}</span>
          {staff ? <SettingsPill tone="accent">team access</SettingsPill>
            : billing.plan && <SettingsPill tone={billing.plan.status === "active" ? "green" : "muted"}>{billing.plan.status}</SettingsPill>}
          {billing.plan?.verifying && <SettingsPill tone="muted">Verifying</SettingsPill>}
          {signedIn && <Button size="sm" variant="ghost" className="ml-auto text-xs" disabled={billing.pending} aria-busy={billing.pending}
            onClick={() => void manageSubscription()}>{paidSubscription || refusedAsSubscribed
              ? billing.pending ? "Opening subscription…" : "Manage subscription" : "Upgrade"}</Button>}
        </div>
        {billing.plan?.periodEnd && <p className={MUTED}>
          {subscriptionPlan === null ? "Access ends" : billing.plan.cancelAtPeriodEnd ? "Ends" : "Renews"} {new Date(billing.plan.periodEnd * 1000).toLocaleDateString()}
        </p>}
        <AccountLimits limits={billing.plan?.limits ?? null} usage={usage} />
        {identity && <GiftRedemption key={identity} identity={identity} verifiedPlan={billing.plan?.stale || billing.plan?.verifying ? null : billing.plan?.plan ?? null} />}
        {desktopBilling.subscription && <p role="status">{scheduledPlanCopy(desktopBilling.subscription, seconds => new Date(seconds * 1000).toLocaleDateString())}</p>}
        {signedIn && !desktopBilling.subscription && <p role="status" className={MUTED}>Verifying your subscription status. Refresh plan offers to check again.</p>}
        {offersReady && desktopBilling.error && <InlineErrorNotice error={desktopBilling.error} operation="managing your plan" />}
        {signedIn && <Button type="button" size="sm" variant="ghost" onClick={desktopBilling.refresh}>Refresh plan offers</Button>}
        {!signedIn && <p className={MUTED}>You’ll sign in when you choose a plan.</p>}
        <div className="mt-3 w-full">
          <SettingsCardAnchor id="subscription-plans" focused={focusCard === "subscription-plans"} focusRequest={plansRequest}>
            {signedIn && !desktopBilling.catalog ? <p role="status" className={MUTED}>
              {desktopBilling.loading ? "Loading current plan offers…" : "Refresh plan offers to choose a subscription."}
            </p> : <PlanCards viewRequested={focusCard === "subscription-plans" || plansRequest > 0} catalog={catalog}
              currentPlan={subscriptionPlan === undefined ? billing.plan?.plan ?? null : subscriptionPlan}
              onChangePlan={paidSubscription && !desktopBilling.subscription?.pending_change && !desktopBilling.submitted
                ? (priceId, tier, direction) => { if (model.kind === "signed_in") setPlanChange({ priceId, tier, direction, identity: model.identity.uid }); } : undefined}
              subscriptionInterval={desktopBilling.subscription?.billing_interval ?? null}
              subscribedPlan={subscriptionPlan ?? (refusedAsSubscribed ? "already_subscribed" : null)} onManage={() => void manageSubscription()} manageBusy={billing.pending}
              canPurchase={!billing.pending && !purchase.busy && !(signedIn && (billing.planLoad === "loading" || !offersReady))}
              onChoose={purchase.choose} intent={purchase.intent} onCancel={purchase.cancel}
              signedInAs={signedIn ? model.identity.email ?? model.identity.display_name : null} />}
          </SettingsCardAnchor>
        </div>
        {planChange && model.kind === "signed_in" && planChange.identity === model.identity.uid && <PlanChangeDialog selection={planChange} currentTier={billing.plan?.plan ?? (signedIn ? "unavailable" : "free")}
          periodEnd={desktopBilling.subscription?.period_end ?? null} pending={desktopBilling.changing} error={desktopBilling.error}
          submitted={desktopBilling.submitted} onClose={() => setPlanChange(null)}
          onSubmit={() => { void desktopBilling.change(planChange.priceId).then(changed => { if (changed) setPlanChange(null); }); }} />}
      </div>
    </SettingsCard>
  </SettingsPage>;
}
