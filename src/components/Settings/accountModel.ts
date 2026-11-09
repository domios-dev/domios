export type AccountIdentity = Omit<import("../../../gen/AccountIdentity").AccountIdentity, "photo_url"> & Partial<Pick<import("../../../gen/AccountIdentity").AccountIdentity, "photo_url">>;
export type SignInStage = import("../../../gen/SignInStage").SignInStage;
export type SignInError = import("../../../gen/AccountError").AccountError;
export type TargetMismatch = import("../../../gen/TargetMismatch").TargetMismatch;
export type SignInFailure = import("../../../gen/SignInFailure").SignInFailure;
export type AccountStatus = Omit<Partial<import("../../../gen/AccountStatus").AccountStatus>, "identity" | "pending" | "target_mismatch"> & {identity: AccountIdentity | null; pending: import("../../../gen/AccountStatus").AccountStatus["pending"]; target_mismatch?: import("../../../gen/AccountStatus").AccountStatus["target_mismatch"] | null};
export type AccountModel =
  | { kind: "signed_out"; sessionRevoked?: boolean; targetMismatch?: string }
  | { kind: "pending" }
  | { kind: "signed_in"; identity: AccountIdentity; sessionOnly?: boolean }
  | { kind: "error"; message: string; failure?: SignInFailure };
export const SESSION_REVOKED_MESSAGE = "This machine was signed out from your account";

export function accountModel(status: AccountStatus): AccountModel {
  if (status.pending) return { kind: "pending" };
  if (status.last_sign_in_failure) return { kind: "error", message: signInFailureMessage(status.last_sign_in_failure), failure: status.last_sign_in_failure };
  if (status.identity) return { kind: "signed_in", identity: status.identity, ...(status.session_only ? { sessionOnly: true } : {}) };
  return { kind: "signed_out", ...(status.session_revoked ? { sessionRevoked: true } : {}), ...(status.target_mismatch ? { targetMismatch: status.target_mismatch.reason } : {}) };
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


export type LoginHint = Partial<import("../../../gen/LoginHint").LoginHint>;
export function accountLoginPath(hint?: LoginHint): string {
  const query = new URLSearchParams();
  if (hint?.provider) query.set("provider", hint.provider);
  if (hint?.mode) query.set("mode", hint.mode);
  return `/api/account/login${query.size ? `?${query}` : ""}`;
}
