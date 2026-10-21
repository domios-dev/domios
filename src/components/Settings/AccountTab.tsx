import { useEffect, useRef } from "react";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { CreditCard, UserRound } from "lucide-react";
import { Button } from "../shared/Button";
import { PlanCards } from "../billing/PlanCards";
import { ACCOUNT_ENTRY_OPTIONS, SIGN_IN_PROVIDER_OPTIONS } from "./accountSignInOptions";
import { CopyRow, SettingsCard, SettingsPage, SettingsPill } from "./shared";
import { useAccountPlan } from "./useAccountPlan";
import { useAccount } from "./useAccount";
import { useMultiplexer } from "../../store/multiplexer";
import { billingFailureToast, signInFailureToast } from "./accountFailureToast";
import type { LoginHint } from "./accountModel";

const MUTED = "m-0 text-xs text-[var(--ct-muted-fg)]";

function initialsOf(name: string): string {
  const parts = name.replace(/@.*/, "").split(/[\s._-]+/).filter(Boolean);
  return (parts.length > 1 ? parts[0][0] + parts[1][0] : name.slice(0, 2)).toUpperCase();
}

export function AccountTab() {
  const { model, signIn, signOut } = useAccount();
  const billing = useAccountPlan(model.kind === "signed_in" ? model.identity.uid : null);
  const { upsertToast } = useMultiplexer();
  const retryHint = useRef<LoginHint | undefined>(undefined);
  const lastSignInFailure = useRef<string | null>(null);
  const signedIn = model.kind === "signed_in";
  useEffect(() => {
    if (model.kind !== "error" || !model.failure) {
      lastSignInFailure.current = null;
      return;
    }
    const key = `${model.failure.stage}:${model.failure.error}`;
    if (lastSignInFailure.current === key) return;
    lastSignInFailure.current = key;
    upsertToast(signInFailureToast(model.failure, () => { void signIn(retryHint.current); }));
  }, [model, signIn, upsertToast]);
  const showBillingFailure = (error: unknown, operation: string, retry: () => void | Promise<void>) => {
    upsertToast(billingFailureToast(error, operation, billing.plan?.status, retry));
  };
  const manageSubscription = async () => {
    const failure = await billing.manage();
    if (failure) showBillingFailure(failure, "managing your subscription", () => manageSubscription());
  };
  const choosePlan = async (planKey: string) => {
    const failure = await billing.upgrade(planKey);
    if (failure) showBillingFailure(failure, "changing your subscription", () => choosePlan(planKey));
  };
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
              {initialsOf(name)}
            </span>
            <div className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-sm font-medium text-[var(--ct-fg)]">{name}</span>
              {model.identity.email && model.identity.email !== name && <span className={`${MUTED} truncate`}>{model.identity.email}</span>}
            </div>
            <Button size="sm" variant="ghost" className="text-xs" onClick={() => void signOut()}>Sign out</Button>
          </div>
          {model.sessionOnly && <p role="status" className={MUTED}>Signed in for this session only: secure storage is unavailable, so sign in again after restarting Domios.</p>}
          <CopyRow label="Account ID" value={model.identity.uid} />
        </div>;
      })() : <div className="flex flex-col items-start gap-3 text-xs">
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
          <span className="text-xl font-semibold capitalize text-[var(--ct-fg)]">{billing.plan?.plan ?? "free"}</span>
          {billing.plan && <SettingsPill tone={billing.plan.status === "active" ? "green" : "muted"}>{billing.plan.status}</SettingsPill>}
          {signedIn && <Button size="sm" variant="ghost" className="ml-auto text-xs" onClick={() => void manageSubscription()}>Manage subscription</Button>}
        </div>
        {billing.plan?.periodEnd && <p className={MUTED}>
          {billing.plan.cancelAtPeriodEnd ? "Ends" : "Renews"} {new Date(billing.plan.periodEnd * 1000).toLocaleDateString()}
        </p>}
        {billing.plan?.stale && <p role="status" className={MUTED}>Showing the last known plan.</p>}
        {!signedIn && <p className={MUTED}>Sign in to change your plan.</p>}
        <div className="w-full">
          <PlanCards currentPlan={billing.plan?.plan ?? null} canPurchase={signedIn} onChoose={(planKey) => void choosePlan(planKey)} />
        </div>
      </div>
    </SettingsCard>
  </SettingsPage>;
}
