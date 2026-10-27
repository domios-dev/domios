import { accountInitials, type AccountModel } from "../Settings/accountModel";
import type { PlanModel } from "../Settings/accountPlanModel";

const MAX_AVATAR_URL_LENGTH = 2048;

const capitalized = (value: string) => value.charAt(0).toUpperCase() + value.slice(1);

export function accountAvatarUrl(value: string | null | undefined): string | null {
  if (!value || value.length > MAX_AVATAR_URL_LENGTH) return null;
  try {
    const url = new URL(value);
    return url.protocol === "https:" && url.hostname && !url.username && !url.password ? url.href : null;
  } catch {
    return null;
  }
}

export interface PlanBadge { tier: string; name: string; admin: boolean; stale: boolean }

export function planBadge(plan: PlanModel | null): PlanBadge | null {
  if (!plan) return null;
  return { tier: plan.plan, name: capitalized(plan.plan), admin: plan.authority === "admin", stale: plan.stale };
}

export function accountTrayModel(account: AccountModel, plan: PlanModel | null) {
  if (account.kind === "signed_in") {
    const identity = account.identity.display_name?.trim() || account.identity.email || account.identity.uid;
    return { kind: "signed_in" as const, identity, email: account.identity.email,
      initial: accountInitials(identity),
      avatarUrl: accountAvatarUrl(account.identity.photo_url),
      badge: planBadge(plan),
      plan: plan ? `${plan.authority === "admin" ? "Admin · " : ""}${capitalized(plan.plan)} · ${plan.status}${plan.stale ? " · stale" : ""}` : "Plan unavailable",
      name: `Account: ${identity}` };
  }
  return { kind: account.kind, name: account.kind === "pending" ? "Signing in or loading account" : "Account",
    disabled: account.kind === "pending", error: account.kind === "error" ? account.message : null };
}
