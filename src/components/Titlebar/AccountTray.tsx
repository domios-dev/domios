import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useState } from "react";
import { UserRound } from "lucide-react";
import { ACCOUNT_ENTRY_OPTIONS, SIGN_IN_PROVIDER_OPTIONS, type SignInOption } from "../Settings/accountSignInOptions";
import { useAccount } from "../Settings/useAccount";
import { useAccountPlan } from "../Settings/useAccountPlan";
import { DropdownMenu, type DropdownMenuEntry } from "../shared/DropdownMenu";
import { Skeleton, SkeletonBlock } from "../shared/Skeleton";
import { UI_TRAY_BTN } from "../shared/design";
import { communityMenu } from "../shared/CommunityLinks";
import { accountTrayModel } from "./accountTrayModel";

export function AccountTray() {
  const account = useAccount();
  const billing = useAccountPlan(account.model.kind === "signed_in" ? account.model.identity.uid : null);
  const model = accountTrayModel(account.model, billing.plan);
  const [open, setOpen] = useState(false);
  const pending = model.kind === "pending";
  const signInEntry = (option: SignInOption): DropdownMenuEntry =>
    ({ label: option.label, icon: option.icon, disabled: pending, onClick: () => void account.signIn(option.hint) });
  const items: DropdownMenuEntry[] = model.kind === "signed_in" ? [
    { label: model.identity, description: model.email ?? undefined, disabled: true },
    { label: model.plan, disabled: true },
    { label: "Manage subscription", onClick: () => void billing.manage() },
    { label: "Sign out", onClick: () => void account.signOut() },
  ] : pending ? [
    { label: "Loading account…", disabled: true },
  ] : [
    ...ACCOUNT_ENTRY_OPTIONS.map(signInEntry),
    { separator: true, key: "account-methods" },
    ...SIGN_IN_PROVIDER_OPTIONS.map(signInEntry),
  ];
  if (account.model.kind === "signed_in" && account.model.sessionOnly) {
    items.unshift({label: "Signed in for this session only", description: "Secure storage is unavailable. Sign in again after restarting Domios.", disabled: true});
  }
  items.push({separator:true,key:"community"},communityMenu());
  const error = model.kind === "signed_in" ? billing.actionError : model.error;
  return <div className="flex items-center">
    <DropdownMenu menuLabel="Account" items={items} onOpenChange={setOpen} trigger={
      <button type="button" className={UI_TRAY_BTN} aria-label={model.name} title={model.name}
        aria-haspopup="menu" aria-expanded={open} aria-busy={pending}>
        {pending
          ? <Skeleton inline><SkeletonBlock shape="circle" width={20} height={20} /></Skeleton>
          : model.kind === "signed_in"
            ? <span className="flex h-5 w-5 items-center justify-center rounded-full bg-[color-mix(in_srgb,var(--ct-accent)_22%,transparent)] text-[11px] font-semibold text-[var(--ct-accent)]" aria-hidden="true">
              {model.initial}
            </span>
            : <span className="flex h-5 w-5 items-center justify-center rounded-full bg-[color-mix(in_srgb,var(--ct-fg)_10%,transparent)] text-[var(--ct-fg)]" aria-hidden="true">
              <UserRound size={13} />
            </span>}
      </button>
    } />
    {error && <InlineErrorNotice error={error} operation="signing in" blocking />}
  </div>;
}
