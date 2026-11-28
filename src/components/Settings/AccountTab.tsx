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
import { usePurchasesEnabled } from "../billing/purchaseSwitch";
import { useCloudMachinesEnabled } from "../../lib/cloudMachines/availability";
import { CommunityKeysNotice } from "../shared/CommunityLinks";
import { ACCOUNT_ENTRY_OPTIONS, SIGN_IN_PROVIDER_OPTIONS } from "./accountSignInOptions";
import { CopyRow, RemoteLocalOnlyNotice, SettingsCard, SettingsPage, SettingsPill } from "./shared";
import { useSettingsRemoteTarget } from "./useSettingsTarget";
import { useAccountPlan } from "./useAccountPlan";
import { unverifiedPlanNotice } from "./accountPlanModel";
import { useAccount } from "./useAccount";
import { useAccountEnrollmentFailure } from "./useAccountEnrollmentFailure";
import { failureMessage } from "../MeshPanel/accountMachines";
import { SettingsCardAnchor } from "./SettingsCardAnchor";
import type { SettingsCardAnchorId } from "./sections";
import { useMultiplexer } from "../../store/multiplexer/hooks";
import { billingFailureToast } from "./accountFailureToast";
import { SESSION_REVOKED_MESSAGE, accountInitials, type LoginHint } from "./accountModel";
import { MachineJoinPreview, MachineIdentitySaveError } from "../MeshPanel/MachineIdentityControls";
import { useMachineJoin } from "../MeshPanel/MachineIdentityEditor";
import { CloudUsageMeter } from "../MeshPanel/CloudUsageMeter";
import { useCloudUsage } from "../MeshPanel/useCloudMachines";
import { cloudHoursPurchasedEvent } from "../../lib/cloudMachines/analytics";
import { redeemCloudPromo } from "../../lib/cloudMachines/client";
import { recordProductEvent } from "../../lib/productAnalytics";
import { CloudTopUpDialog, type PendingCloudTopUpCheckout } from "../billing/CloudTopUpDialog";
import { CloudTopUpReturnDialog } from "../billing/CloudTopUpReturnDialog";

const MUTED = "m-0 text-xs text-[var(--ct-muted-fg)]";

export function AccountTab(props: { focusCard?: SettingsCardAnchorId; purchases?: boolean }) {
  const remoteTarget = useSettingsRemoteTarget();
  if (remoteTarget) {
    return (
      <SettingsPage title="Account">
        <SettingsCard icon={<UserRound size={16} />} title="Domios account">
          <RemoteLocalOnlyNotice
            machineName={remoteTarget.name}
            subject="Account sign-in and plan settings"
          />
        </SettingsCard>
      </SettingsPage>
    );
  }
  return <LocalAccountTab {...props} />;
}

function LocalAccountTab({ focusCard, purchases: explicitPurchases }: { focusCard?: SettingsCardAnchorId; purchases?: boolean }) {
  const remotePurchases = usePurchasesEnabled();
  const purchases = explicitPurchases ?? remotePurchases;
  const { upsertToast } = useMultiplexer();
  const notify = createNoticePublisher({ upsertToast });
  const { model, signIn, signOut, signingIn, cancelSignIn } = useAccount((message) => notify({
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
  const join = useMachineJoin(model.kind === "signed_out");
  const identity = signedIn ? model.identity.uid : null;
  const enrollmentFailure = useAccountEnrollmentFailure(identity);
  useEffect(() => setRefusedAsSubscribed(false), [identity]);
  const usage = useAccountUsage(identity);
  const cloudEnabled = useCloudMachinesEnabled();
  const cloudUsage = useCloudUsage(signedIn && cloudEnabled);
  const [cloudTopUpOpen, setCloudTopUpOpen] = useState(false);
  const [pendingCloudCheckout, setPendingCloudCheckout] = useState<PendingCloudTopUpCheckout | null>(null);
  const [cloudReturn, setCloudReturn] = useState<{ addedHours: number; newBalance: number; rolloverSentence: string | null } | null>(null);
  useEffect(() => {
    if (!pendingCloudCheckout || !cloudUsage.usage) return;
    const delta = Math.round((cloudUsage.usage.remaining - pendingCloudCheckout.priorRemaining) * 10) / 10;
    if (delta <= 0) return;
    const checkout = pendingCloudCheckout;
    setPendingCloudCheckout(null);
    void recordProductEvent(cloudHoursPurchasedEvent(checkout.packId, checkout.amountCents));
    setCloudReturn({
      addedHours: delta || checkout.expectedHours || 0,
      newBalance: cloudUsage.usage.remaining,
      rolloverSentence: checkout.rolloverSentence,
    });
  }, [pendingCloudCheckout, cloudUsage.usage]);
  const offersReady = desktopBilling.catalog !== null && desktopBilling.subscription !== null;
  const paidSubscription = desktopBilling.subscription?.paid_subscription;
  const subscriptionPlan = paidSubscription === undefined ? undefined : paidSubscription?.tier ?? null;
  const purchase = usePurchaseIntent({
    account: { model, signIn, signingIn },
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
  const unverified = unverifiedPlanNotice(billing.plan?.unverified);
  const startSignIn = async (hint: LoginHint) => {
    retryHint.current = hint;
    if (await join.beforeJoin()) await signIn(hint);
  };
  const activeHint = retryHint.current ?? ACCOUNT_ENTRY_OPTIONS[0].hint;
  const cancel = cancelSignIn && <Button size="sm" onClick={() => void cancelSignIn()}>Cancel</Button>;
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
        {model.kind === "signed_out" && !model.sessionRevoked && model.targetMismatch && <p role="status" className={MUTED}>{model.targetMismatch}</p>}
        <p className={MUTED}>Sign in to keep your plan and settings with your account.</p>
        {model.kind === "error" && !model.failure && <InlineErrorNotice error={model.message} operation="updating your account" blocking />}
        {join.view && join.value && <MachineJoinPreview name={join.view.name} value={join.value} onChange={join.setDraft} />}
        {join.error && retryHint.current && <MachineIdentitySaveError error={join.error} retry={() => { if (retryHint.current) void startSignIn(retryHint.current); }} />}
        <div className="flex flex-wrap items-center gap-2">
          {ACCOUNT_ENTRY_OPTIONS.map((option, index) =>
            <Button key={option.label} size="sm" variant={index === 0 ? "primary" : "ghost"} disabled={model.kind === "pending" || signingIn} loading={signingIn && activeHint === option.hint}
              onClick={() => startSignIn(option.hint)}>{signingIn && activeHint === option.hint ? "Signing in…" : option.label}</Button>)}
          {cancel}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {SIGN_IN_PROVIDER_OPTIONS.map((option) =>
            <Button key={option.label} size="sm" variant="ghost" className="gap-2" disabled={model.kind === "pending" || signingIn} loading={signingIn && activeHint === option.hint}
              onClick={() => startSignIn(option.hint)}>{signingIn && activeHint === option.hint ? "Signing in…" : <>{option.icon}{option.label}</>}</Button>)}
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
          {unverified && <SettingsPill tone="muted">{unverified.label}</SettingsPill>}
          {signedIn && (purchases || paidSubscription || refusedAsSubscribed) && <Button size="sm" variant="ghost" className="ml-auto text-xs" disabled={billing.pending} aria-busy={billing.pending}
            onClick={() => void manageSubscription()}>{paidSubscription || refusedAsSubscribed
              ? billing.pending ? "Opening subscription…" : "Manage subscription" : "Upgrade"}</Button>}
        </div>
        {unverified && <div role="status" className="flex w-full flex-wrap items-center gap-2">
          <p className={MUTED}>{unverified.detail}</p>
          {billing.plan?.unverified === "sign_in_required" && <Button size="sm" variant="outline" loading={signingIn}
            onClick={() => startSignIn(ACCOUNT_ENTRY_OPTIONS[0].hint)}>{signingIn ? "Signing in…" : "Sign in again"}</Button>}
          {signedIn && cancel}
        </div>}
        {billing.plan?.periodEnd && <p className={MUTED}>
          {subscriptionPlan === null ? "Access ends" : billing.plan.cancelAtPeriodEnd ? "Ends" : "Renews"} {new Date(billing.plan.periodEnd * 1000).toLocaleDateString()}
        </p>}
        <AccountLimits limits={billing.plan?.limits ?? null} usage={usage} />
        {signedIn && cloudUsage.usage && (
          <div className="mt-2 w-full border-t border-[var(--ct-border-subtle)] pt-3">
            <CloudUsageMeter
              usage={cloudUsage.usage}
              planId={billing.plan?.plan ?? null}
              onTopUp={() => setCloudTopUpOpen(true)}
              onRedeem={async (code) => {
                const reply = await redeemCloudPromo(code);
                cloudUsage.applyRedemption(reply);
                void cloudUsage.refresh();
                return reply;
              }}
            />
          </div>
        )}
        {!purchases && <div className="mt-1 w-full">
          <SettingsCardAnchor id="subscription-plans" focused={focusCard === "subscription-plans"} focusRequest={plansRequest}>
            <CommunityKeysNotice />
          </SettingsCardAnchor>
        </div>}
        {identity && <GiftRedemption key={identity} identity={identity} verifiedPlan={billing.plan?.stale || billing.plan?.verifying || billing.plan?.unverified ? null : billing.plan?.plan ?? null} />}
        {!purchases && !signedIn && <p className={MUTED}>Sign in to redeem a key.</p>}
        {desktopBilling.subscription && <p role="status">{scheduledPlanCopy(desktopBilling.subscription, seconds => new Date(seconds * 1000).toLocaleDateString())}</p>}
        {purchases && signedIn && !desktopBilling.subscription && <p role="status" className={MUTED}>Verifying your subscription status. Refresh plan offers to check again.</p>}
        {offersReady && desktopBilling.error && <InlineErrorNotice error={desktopBilling.error} operation="managing your plan" />}
        {purchases && signedIn && <Button type="button" size="sm" variant="ghost" onClick={desktopBilling.refresh}>Refresh plan offers</Button>}
        {purchases && !signedIn && <p className={MUTED}>You’ll sign in when you choose a plan.</p>}
        {purchases && <div className="mt-3 w-full">
          <SettingsCardAnchor id="subscription-plans" focused={focusCard === "subscription-plans"} focusRequest={plansRequest}>
            {signedIn && !desktopBilling.catalog ? <p role="status" className={MUTED}>
              {desktopBilling.loading ? "Loading current plan offers…" : "Refresh plan offers to choose a subscription."}
            </p> : <PlanCards viewRequested={focusCard === "subscription-plans" || plansRequest > 0} catalog={catalog}
              currentPlan={subscriptionPlan === undefined ? billing.plan?.plan ?? null : subscriptionPlan}
              onChangePlan={paidSubscription && !desktopBilling.subscription?.pending_change && !desktopBilling.submitted
                ? (priceId, tier, direction) => { if (model.kind === "signed_in") setPlanChange({ priceId, tier, direction, identity: model.identity.uid }); } : undefined}
              subscriptionInterval={desktopBilling.subscription?.billing_interval ?? null}
              planChangeMode={desktopBilling.subscription?.plan_change_mode ?? null}
              subscribedPlan={subscriptionPlan ?? (refusedAsSubscribed ? "already_subscribed" : null)} onManage={() => void manageSubscription()} manageBusy={billing.pending}
              canPurchase={!billing.pending && !purchase.busy && !(signedIn && (billing.planLoad === "loading" || !offersReady))}
              onChoose={purchase.choose} intent={purchase.intent} onCancel={purchase.cancel}
              signedInAs={signedIn ? model.identity.email ?? model.identity.display_name : null} />}
          </SettingsCardAnchor>
        </div>}
        {purchases && planChange && paidSubscription && model.kind === "signed_in" && planChange.identity === model.identity.uid && <PlanChangeDialog selection={planChange} currentTier={paidSubscription.tier}
          periodEnd={paidSubscription.period_end} planChangeMode={desktopBilling.subscription?.plan_change_mode ?? null} pending={desktopBilling.changing} error={desktopBilling.error}
          submitted={desktopBilling.submitted} onClose={() => setPlanChange(null)}
          onSubmit={() => { void desktopBilling.change(planChange.priceId).then(changed => { if (changed) setPlanChange(null); }); }} />}
        {cloudEnabled && cloudTopUpOpen && (
          <CloudTopUpDialog
            mode={billing.plan?.plan === "free" && !staff ? "free" : "topup"}
            usage={cloudUsage.usage}
            currentPlan={billing.plan?.plan ?? null}
            purchases={purchases}
            onClose={() => setCloudTopUpOpen(false)}
            onOpenPlans={() => {
              setCloudTopUpOpen(false);
              revealPlans();
            }}
            onCheckoutStarted={(pending) => setPendingCloudCheckout(pending)}
          />
        )}
        {cloudEnabled && cloudReturn && (
          <CloudTopUpReturnDialog
            addedHours={cloudReturn.addedHours}
            newBalance={cloudReturn.newBalance}
            rolloverSentence={cloudReturn.rolloverSentence}
            onClose={() => setCloudReturn(null)}
          />
        )}
      </div>
    </SettingsCard>
  </SettingsPage>;
}
