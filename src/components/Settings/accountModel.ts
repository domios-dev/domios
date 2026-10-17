export interface AccountIdentity {
  uid: string;
  email: string | null;
  display_name: string | null;
  provider: string;
}
export interface AccountStatus {
  identity: AccountIdentity | null;
  pending: boolean;
  session_only?: boolean;
}
export type AccountModel =
  | { kind: "signed_out" }
  | { kind: "pending" }
  | { kind: "signed_in"; identity: AccountIdentity; sessionOnly?: boolean }
  | { kind: "error"; message: string };
export function accountModel(status: AccountStatus): AccountModel {
  if (status.pending) return { kind: "pending" };
  return status.identity ? { kind: "signed_in", identity: status.identity, ...(status.session_only ? { sessionOnly: true } : {}) } : { kind: "signed_out" };
}
export function accountFailure(error: unknown): AccountModel {
  return { kind: "error", message: error instanceof Error ? error.message : "Account request failed" };
}


export interface LoginHint {
  provider?: "google" | "facebook" | "email";
  mode?: "signin" | "signup";
}
export function accountLoginPath(hint?: LoginHint): string {
  const query = new URLSearchParams();
  if (hint?.provider) query.set("provider", hint.provider);
  if (hint?.mode) query.set("mode", hint.mode);
  return `/api/account/login${query.size ? `?${query}` : ""}`;
}
