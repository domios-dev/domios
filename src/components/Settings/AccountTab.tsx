import { UserRound } from "lucide-react";
import { SettingsCard, SettingsPage } from "./shared";
import { useAccountPlan } from "./useAccountPlan";
import { useAccount } from "./useAccount";
export function AccountTab() {
  const { model, signIn, signOut } = useAccount();
  const billing = useAccountPlan(model.kind === "signed_in" ? model.identity.uid : null);
  return <SettingsPage title="Account">
    <SettingsCard icon={<UserRound size={16} />} title="Domios account" description="Your identity on this machine">
      {model.kind === "signed_in" && <div>
        <p>{model.identity.display_name ?? model.identity.email ?? model.identity.uid}</p>
        {model.identity.email && <p>{model.identity.email}</p>}
        <p>UID: {model.identity.uid}</p>
        <button type="button" onClick={() => void signOut()}>Sign out</button>
      </div>}
      {model.kind === "pending" && <p role="status">Signing in or loading account…</p>}
      {model.kind === "error" && <p role="alert">{model.message}</p>}
      {(model.kind === "signed_out" || model.kind === "error") && <button type="button" onClick={() => void signIn()}>Sign in</button>}
    </SettingsCard>
    <SettingsCard icon={<UserRound size={16} />} title="Subscription" description="Your Domios plan">
      {billing.plan && <div>
        <p>Plan: {billing.plan.plan} ({billing.plan.status})</p>
        {billing.plan.periodEnd && <p>Current period ends: {new Date(billing.plan.periodEnd * 1000).toLocaleDateString()}</p>}
        {billing.plan.cancelAtPeriodEnd && <p>Cancels at the end of this period</p>}
        {billing.plan.stale && <p role="status">Plan information is stale</p>}
      </div>}
      {billing.error && <p role="alert">{billing.error}</p>}
      <button type="button" disabled={model.kind !== "signed_in"} onClick={() => void billing.upgrade("pro")}>Upgrade</button>
      <button type="button" disabled={model.kind !== "signed_in"} onClick={() => void billing.manage()}>Manage subscription</button>
    </SettingsCard>
  </SettingsPage>;
}
