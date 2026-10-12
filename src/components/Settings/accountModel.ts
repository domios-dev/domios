export interface AccountIdentity {
  uid: string;
  email: string | null;
  display_name: string | null;
  provider: string;
}
export interface AccountStatus {
  identity: AccountIdentity | null;
  pending: boolean;
}
export type AccountModel =
  | { kind: "signed_out" }
  | { kind: "pending" }
  | { kind: "signed_in"; identity: AccountIdentity }
  | { kind: "error"; message: string };
export function accountModel(status: AccountStatus): AccountModel {
  if (status.pending) return { kind: "pending" };
  return status.identity ? { kind: "signed_in", identity: status.identity } : { kind: "signed_out" };
}
export function accountFailure(error: unknown): AccountModel {
  return { kind: "error", message: error instanceof Error ? error.message : "Account request failed" };
}
