import { useState } from "react";
import { ChevronDown, UserRound } from "lucide-react";
import { useAccount } from "../Settings/useAccount";
import { useAccountPlan } from "../Settings/useAccountPlan";
import { DropdownMenu, type DropdownMenuEntry } from "../shared/DropdownMenu";
import { UI_TRAY_BTN } from "../shared/design";
import { communityMenu } from "../shared/CommunityLinks";
import { accountTrayModel } from "./accountTrayModel";

export function AccountTray() {
  const account = useAccount();
  const billing = useAccountPlan(account.model.kind === "signed_in" ? account.model.identity.uid : null);
  const model = accountTrayModel(account.model, billing.plan);
  const [open, setOpen] = useState(false);
  const pending = model.kind === "pending";
  const items: DropdownMenuEntry[] = model.kind === "signed_in" ? [
    { label: model.identity, description: model.email ?? undefined, disabled: true },
    { label: model.plan, disabled: true },
    { label: "Manage subscription", onClick: () => void billing.manage() },
    { label: "Sign out", onClick: () => void account.signOut() },
  ] : [
    { label: "Sign in", disabled: pending, onClick: () => void account.signIn({mode:"signin"}) },
    { label: "Create account", disabled: pending, onClick: () => void account.signIn({mode:"signup"}) },
    { separator: true, key: "account-methods" },
    { label: "Continue with Google", disabled: pending, onClick: () => void account.signIn({provider:"google",mode:"signin"}) },
    { label: "Continue with Facebook", disabled: pending, onClick: () => void account.signIn({provider:"facebook",mode:"signin"}) },
    { label: "Continue with email", disabled: pending, onClick: () => void account.signIn({provider:"email",mode:"signin"}) },
  ];
  items.push({separator:true,key:"community"},communityMenu());
  const error = model.kind === "signed_in" ? billing.error : model.error;
  return <div className="flex items-center">
    <DropdownMenu menuLabel="Account" items={items} onOpenChange={setOpen} trigger={
      <button type="button" className={UI_TRAY_BTN} aria-label={model.name} title={model.name}
        aria-haspopup="menu" aria-expanded={open} aria-busy={pending}>
        <span className="flex h-4 w-4 items-center justify-center rounded-full border border-[var(--ct-border-subtle)] text-[11px]" aria-hidden="true">
          {model.kind === "signed_in" ? model.initial : <UserRound size={13} />}
        </span>
        <ChevronDown size={12} className="shrink-0 text-[var(--ct-muted-fg)]" aria-hidden="true" />
      </button>
    } />
    {error && <span role="alert" className="text-[11px] text-[var(--ct-text-muted)]">{error}</span>}
  </div>;
}
