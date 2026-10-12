import type { AccountModel } from "../Settings/accountModel";
import type { PlanModel } from "../Settings/accountPlanModel";

export function accountTrayModel(account: AccountModel, plan: PlanModel | null) {
  if (account.kind === "signed_in") {
    const identity = account.identity.display_name?.trim() || account.identity.email || account.identity.uid;
    return { kind: "signed_in" as const, identity, email: account.identity.email,
      initial: Array.from(identity)[0]?.toLocaleUpperCase() || "?",
      plan: plan ? `${plan.plan} (${plan.status})${plan.stale ? " · stale" : ""}` : "Plan unavailable",
      name: `Account: ${identity}` };
  }
  return { kind: account.kind, name: account.kind === "pending" ? "Signing in or loading account" : "Sign in",
    disabled: account.kind === "pending", error: account.kind === "error" ? account.message : null };
}
