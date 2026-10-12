import { useState } from "react";
import { useAccount } from "../Settings/useAccount";
import { useAccountPlan } from "../Settings/useAccountPlan";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { UI_TRAY_BTN } from "../shared/design";
import { accountTrayModel } from "./accountTrayModel";

export function AccountTray() {
  const account = useAccount();
  const billing = useAccountPlan(account.model.kind === "signed_in" ? account.model.identity.uid : null);
  const model = accountTrayModel(account.model, billing.plan);
  const [open, setOpen] = useState(false);
  if (model.kind !== "signed_in") return <div className="flex items-center">
    <button type="button" className={UI_TRAY_BTN} aria-label={model.name} disabled={model.disabled}
      onClick={() => void account.signIn()}>{model.disabled ? "…" : "Sign in"}</button>
    {model.error && <span role="alert" className="text-[11px] text-[var(--ct-text-muted)]">{model.error}</span>}
  </div>;
  const items: DropdownMenuItem[] = [
    { label: model.identity, description: model.email ?? undefined, disabled: true },
    { label: model.plan, disabled: true },
    { label: "Manage subscription", onClick: () => void billing.manage() },
    { label: "Sign out", onClick: () => void account.signOut() },
  ];
  return <div className="flex items-center">
    <DropdownMenu menuLabel="Account" items={items} onOpenChange={setOpen} trigger={
      <button type="button" className={UI_TRAY_BTN} aria-label={model.name} title={model.identity}
        aria-haspopup="menu" aria-expanded={open}>{model.initial}</button>
    } />
    {billing.error && <span role="alert" className="text-[11px] text-[var(--ct-text-muted)]">{billing.error}</span>}
  </div>;
}
