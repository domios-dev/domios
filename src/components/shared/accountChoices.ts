import type { ProviderAccountRow, ProviderAccountsSnapshot } from "../../hooks/providerAccountsTypes";

export function accountChoiceName(account: ProviderAccountRow, providerName: string): string {
  return account.name ?? account.identity ?? providerName;
}

export function accountChoiceDescription(account: ProviderAccountRow): string | undefined {
  if (account.status === "signedOut") return "Signed out";
  if (account.status === "checking") return "Checking sign-in";
  if (account.status === "probeFailed") return "Couldn't check sign-in";
  if (account.status === "signingIn") return "Waiting for sign-in";
  if (account.status === "exhausted") {
    return account.resetsAtMs == null ? "Limit reached" : `Limit reached, resets ${new Date(account.resetsAtMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
  }
  if (account.status === "ready") return account.tabsInUse > 0
    ? `In use by ${account.tabsInUse} ${account.tabsInUse === 1 ? "tab" : "tabs"}` : account.plan;
  return undefined;
}

export function accountChoices(snapshot: ProviderAccountsSnapshot) {
  return snapshot.accounts.filter(account => account.status !== "removing");
}
