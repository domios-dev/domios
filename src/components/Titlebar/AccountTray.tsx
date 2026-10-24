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
import { accountTrayModel, type PlanBadge } from "./accountTrayModel";
import { SelfSessionSignOut } from "./SelfSessionSignOut";
import { SESSION_REVOKED_MESSAGE } from "../Settings/accountModel";
import { AccountAvatar } from "./AccountAvatar";
import { FIRST_RUN_DISABLED_REASON } from "../../lib/firstRunGate";
import { useMultiplexer } from "../../store/multiplexer/context";

/** Who is signed in and on which plan: information, not actions, so it sits above the menu items and never takes focus. */
function AccountHeader({ identity, email, avatarUrl, initial, badge, summary }: {
  identity: string; email: string | null; avatarUrl: string | null; initial: string; badge: PlanBadge | null; summary: string;
}) {
  return <div className="ct-menu-head ct-account-head" role="group" aria-label={`${identity}${email && email !== identity ? `, ${email}` : ""}. ${summary}`} data-testid="account-menu-head">
    <div className="ct-menu-identity">
      <AccountAvatar url={avatarUrl} initial={initial} label="" size="menu" />
      <span className="ct-menu-identity__text">
        <span className="ct-menu-identity__title">{identity}</span>
        {email && email !== identity && <span className="ct-menu-identity__subtitle">{email}</span>}
      </span>
    </div>
    <div className="ct-account-plan">
      {badge ? <>
        <span className="ct-plan-badge" data-tier={badge.tier}>{badge.admin ? `Admin · ${badge.name}` : badge.name}</span>
        <span className="ct-plan-status" data-tone={badge.tone}><span className="ct-plan-status__dot" aria-hidden="true" />{badge.status}</span>
        {badge.stale && <span className="ct-plan-status" data-tone="inactive">last known</span>}
      </> : <span className="ct-plan-status" data-tone="inactive">Plan unavailable</span>}
    </div>
  </div>;
}

export function AccountTray({ onOpenPlans, locked = false }: { onOpenPlans: () => void; locked?: boolean }) {
  const account = useAccount();
  const { upsertToast } = useMultiplexer();
  const billing = useAccountPlan(account.model.kind === "signed_in" ? account.model.identity.uid : null);
  const model = accountTrayModel(account.model, billing.plan);
  const [open, setOpen] = useState(false);
  const [confirmSignOut, setConfirmSignOut] = useState(false);
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
    { label: billing.pending ? "Opening subscription…" : "Manage subscription", disabled: billing.pending, onClick: () => void manageSubscription() },
    ...legalMenuItems(["terms", "privacy"]),
    { separator: true, key: "sign-out" },
    { label: "Sign out this machine", onClick: () => setConfirmSignOut(true) },
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
  if (locked && model.kind === "signed_in") {
    for (const item of items) {
      if (!("separator" in item) && !("items" in item)) {
        item.disabled = true;
        item.title = FIRST_RUN_DISABLED_REASON;
      }
    }
  }
  const community = communityMenu();
  if (locked) community.items = community.items.map((item) => ({ ...item, disabled: true, title: FIRST_RUN_DISABLED_REASON }));
  items.push({separator:true,key:"community"},community);
  return <div className="flex items-center">
    {confirmSignOut && <SelfSessionSignOut onClose={() => setConfirmSignOut(false)} />}
    <DropdownMenu menuLabel={model.kind === "signed_in" ? `Account: ${model.identity}, ${model.plan}` : "Account"} items={items} onOpenChange={setOpen}
      header={model.kind === "signed_in" ? <AccountHeader identity={model.identity} email={model.email} avatarUrl={model.avatarUrl} initial={model.initial} badge={model.badge} summary={model.plan} /> : account.model.kind === "signed_out" && account.model.sessionRevoked ? <p role="status" className="px-3 py-2 text-xs">{SESSION_REVOKED_MESSAGE}</p> : undefined} trigger={
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
