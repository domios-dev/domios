import { useState } from "react";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { CreditCard, UserRound } from "lucide-react";
import { Button } from "../shared/Button";
import { PlanCards } from "../billing/PlanCards";
import { ACCOUNT_ENTRY_OPTIONS, SIGN_IN_PROVIDER_OPTIONS } from "./accountSignInOptions";
import { CopyRow, SettingsCard, SettingsPage, SettingsPill } from "./shared";
import { useAccountPlan } from "./useAccountPlan";
import { useAccount } from "./useAccount";
import { SettingsCardAnchor } from "./SettingsCardAnchor";
import type { SettingsCardAnchorId } from "./sections";

const MUTED = "m-0 text-xs text-[var(--ct-muted-fg)]";

function initialsOf(name: string): string {
  const parts = name.replace(/@.*/, "").split(/[\s._-]+/).filter(Boolean);
  return (parts.length > 1 ? parts[0][0] + parts[1][0] : name.slice(0, 2)).toUpperCase();
}

export function AccountTab({ focusCard }: { focusCard?: SettingsCardAnchorId }) {
  const { model, signIn, signOut } = useAccount();
  const billing = useAccountPlan(model.kind === "signed_in" ? model.identity.uid : null);
  const [plansRequest, setPlansRequest] = useState(0);
  const signedIn = model.kind === "signed_in";
  const revealPlans = () => setPlansRequest((count) => count + 1);
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
        {model.kind === "error" && <InlineErrorNotice error={model.message} operation="signing in" blocking />}
        <div className="flex flex-wrap items-center gap-2">
          {ACCOUNT_ENTRY_OPTIONS.map((option, index) =>
            <Button key={option.label} size="sm" variant={index === 0 ? "primary" : "ghost"} disabled={model.kind === "pending"}
              onClick={() => void signIn(option.hint)}>{option.label}</Button>)}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {SIGN_IN_PROVIDER_OPTIONS.map((option) =>
            <Button key={option.label} size="sm" variant="ghost" className="gap-2" disabled={model.kind === "pending"}
              onClick={() => void signIn(option.hint)}>{option.icon}{option.label}</Button>)}
        </div>
      </div>}
    </SettingsCard>
    <SettingsCard icon={<CreditCard size={16} />} title="Plan">
      <div className="flex flex-col items-start gap-3">
        <div className="flex w-full items-center gap-2">
          <span className="text-xl font-semibold capitalize text-[var(--ct-fg)]">{billing.plan?.plan ?? "free"}</span>
          {billing.plan && <SettingsPill tone={billing.plan.status === "active" ? "green" : "muted"}>{billing.plan.status}</SettingsPill>}
          {signedIn && <Button size="sm" variant="ghost" className="ml-auto text-xs" disabled={billing.pending} aria-busy={billing.pending}
            onClick={() => void billing.manage(revealPlans)}>{billing.pending ? "Opening subscription…" : "Manage subscription"}</Button>}
        </div>
        {billing.plan?.periodEnd && <p className={MUTED}>
          {billing.plan.cancelAtPeriodEnd ? "Ends" : "Renews"} {new Date(billing.plan.periodEnd * 1000).toLocaleDateString()}
        </p>}
        {billing.plan?.stale && <p role="status" className={MUTED}>Showing the last known plan.</p>}
        {billing.actionError && <InlineErrorNotice error={billing.actionError} operation="loading your plan" blocking />}
        {!signedIn && <p className={MUTED}>Sign in to change your plan.</p>}
        <div className="w-full">
          <SettingsCardAnchor id="subscription-plans" focused={focusCard === "subscription-plans"} focusRequest={plansRequest}>
            <PlanCards currentPlan={billing.plan?.plan ?? null} canPurchase={signedIn && !billing.pending} onChoose={(planKey) => void billing.upgrade(planKey)} />
          </SettingsCardAnchor>
        </div>
      </div>
    </SettingsCard>
  </SettingsPage>;
}
