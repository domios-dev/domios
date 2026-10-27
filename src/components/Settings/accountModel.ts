export interface AccountIdentity {
  uid: string;
  email: string | null;
  display_name: string | null;
  provider: string;
  photo_url?: string | null;
}
export type SignInStage = "preparation" | "browser" | "callback" | "redemption" | "credential_save" | "publication";
export type SignInError = "configuration" | "invalid_callback" | "wrong_state" | "consumed" | "timeout" | "cancelled" | "pending" | "transport" | "provider" | "credentials" | "browser";
export interface SignInFailure {
  stage: SignInStage;
  error: SignInError;
}
export interface AccountStatus {
  identity: AccountIdentity | null;
  pending: boolean;
  session_only?: boolean;
  session_revoked?: boolean;
  server_stop_unconfirmed?: boolean;
  last_sign_in_failure?: SignInFailure | null;
}
export type AccountModel =
  | { kind: "signed_out"; sessionRevoked?: boolean }
  | { kind: "pending" }
  | { kind: "signed_in"; identity: AccountIdentity; sessionOnly?: boolean }
  | { kind: "error"; message: string; failure?: SignInFailure };
export const SESSION_REVOKED_MESSAGE = "This machine was signed out from your account";

export function accountModel(status: AccountStatus): AccountModel {
  if (status.pending) return { kind: "pending" };
  if (status.last_sign_in_failure) return { kind: "error", message: signInFailureMessage(status.last_sign_in_failure), failure: status.last_sign_in_failure };
  return status.identity ? { kind: "signed_in", identity: status.identity, ...(status.session_only ? { sessionOnly: true } : {}) } : { kind: "signed_out", ...(status.session_revoked ? { sessionRevoked: true } : {}) };
}
export function accountInitials(name: string): string {
  const parts = name.replace(/@.*/, "").split(/[\s._-]+/).filter(Boolean);
  return (parts.length > 1 ? parts[0][0] + parts[1][0] : name.slice(0, 2)).toUpperCase();
}
export function accountFailure(_error: unknown): AccountModel {
  return { kind: "error", message: "Could not update your account. Try again." };
}
export function signInFailureMessage(failure: SignInFailure): string {
  if (failure.error === "cancelled") return "Sign-in was canceled. Try signing in again.";
  if (failure.error === "transport") return "Domios could not connect to complete sign-in. Check your connection and try again.";
  const messages: Record<SignInStage, string> = {
    preparation: "Sign-in is unavailable. Update Domios and try again.",
    browser: "Domios could not open the sign-in page. Try signing in again.",
    callback: "Domios did not receive a valid sign-in result. Try signing in again.",
    redemption: "Domios could not finish verifying your sign-in. Try signing in again.",
    credential_save: "Domios could not keep your sign-in session. Try signing in again.",
    publication: "Domios could not finish signing you in. Try signing in again.",
  };
  return messages[failure.stage] ?? "Domios could not finish signing you in. Try again.";
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
