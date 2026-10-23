import { createNoticePublisher } from "../../lib/notices";
import { useState } from "react";
import { UserRound } from "lucide-react";
import { BusyState } from "../shared/BusyState";
import { ACCOUNT_ENTRY_OPTIONS, SIGN_IN_PROVIDER_OPTIONS, type SignInOption } from "../Settings/accountSignInOptions";
import { useAccount } from "../Settings/useAccount";
import { useAccountPlan } from "../Settings/useAccountPlan";
import { billingFailureToast, signInFailureToast } from "../Settings/accountFailureToast";
import { DropdownMenu, type DropdownMenuEntry } from "../shared/DropdownMenu";
import { Skeleton, SkeletonBlock } from "../shared/Skeleton";
import { UI_TRAY_BTN } from "../shared/design";
import { communityMenu } from "../shared/CommunityLinks";
import { legalMenuItems } from "../shared/LegalLinks";
import { accountTrayModel } from "./accountTrayModel";
import { AccountAvatar } from "./AccountAvatar";
import { useMultiplexer } from "../../store/multiplexer/context";

export function AccountTray({ onOpenPlans }: { onOpenPlans: () => void }) {
  const account = useAccount();
  const { upsertToast } = useMultiplexer();
  const billing = useAccountPlan(account.model.kind === "signed_in" ? account.model.identity.uid : null);
  const model = accountTrayModel(account.model, billing.plan);
  const [open, setOpen] = useState(false);
  const pending = model.kind === "pending";
  const startSignIn = async (hint: SignInOption["hint"]) => {
    const failure = await account.signIn(hint);
    if (!failure) return;
    createNoticePublisher({ upsertToast })(signInFailureToast(failure, () => startSignIn(hint)));
  };
  const manageSubscription = async () => {
    const failure = await billing.manage(onOpenPlans);
    if (!failure) return;
    createNoticePublisher({ upsertToast })(billingFailureToast(failure, "managing your subscription", billing.plan?.status,
      () => manageSubscription()));
  };
  const signInEntry = (option: SignInOption): DropdownMenuEntry =>
    ({ label: option.label, icon: option.icon, disabled: pending, onClick: () => void startSignIn(option.hint) });
  const items: DropdownMenuEntry[] = model.kind === "signed_in" ? [
    { label: model.identity, description: model.email ?? undefined, disabled: true,
      icon: <AccountAvatar url={model.avatarUrl} initial={model.initial} label="" size="menu" /> },
    { label: model.plan, disabled: true },
    { label: billing.pending ? "Opening subscription…" : "Manage subscription", disabled: billing.pending, onClick: () => void manageSubscription() },
    ...legalMenuItems(["terms", "privacy"]),
    { separator: true, key: "sign-out" },
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
  return <div className="flex items-center">
    <DropdownMenu menuLabel="Account" items={items} onOpenChange={setOpen} trigger={
      <button type="button" className={UI_TRAY_BTN} aria-label={billing.pending ? "Opening subscription" : model.name}
        title={billing.pending ? "Opening subscription…" : model.name} aria-haspopup="menu" aria-expanded={open} aria-busy={pending || billing.pending}>
        {pending
          ? <Skeleton inline><SkeletonBlock shape="circle" width={20} height={20} /></Skeleton>
          : billing.pending
            ? <BusyState inline hideLabel dotSize="small" label="Opening subscription" />
          : model.kind === "signed_in"
            ? <AccountAvatar url={model.avatarUrl} initial={model.initial} label={`${model.identity} avatar`} size="tray" />
            : <span className="flex h-5 w-5 items-center justify-center rounded-full bg-[color-mix(in_srgb,var(--ct-fg)_10%,transparent)] text-[var(--ct-fg)]" aria-hidden="true">
              <UserRound size={13} />
            </span>}
      </button>
    } />
  </div>;
}
