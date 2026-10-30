import type { Notification } from "../../lib/notificationTypes";
import { DEV_BUILD } from "../../lib/devBuild";
import type { AccountMachineRow, AccountMeshFailure, AccountMeshSnapshot, LocalPairFact, MeshTarget } from "./accountMeshClient";

export const SIGNED_OUT_COPY = "Signed out — sign in to use the mesh";
export const SIGN_IN_COPY = "Sign in on each machine with the same Domios account to connect automatically.";
export const REAUTH_REQUIRED_COPY = "This machine needs a fresh sign-in to rejoin your mesh.";
export const PLAN_LOCKED_COPY = "Locked — this machine is outside your plan's machine limit";
export type AccountPresence = "pending" | "signed_out" | "signed_in";
export type MachineStatus = "signed_out" | "linked" | "linking" | "endpoint_missing" | "endpoint_stale" | "expired" | "revoked" | "over_limit" | "unverified" | "blocked" | "other_account";
export interface MachineControls { plans?: boolean; revoke: MeshTarget | null }
export interface MachineRowView {
  canSignOut?: boolean;
  transport?: import("./networkTransport").NetworkIdentity;
  key: string;
  name: string;
  status: MachineStatus;
  detail: string;
  online: boolean | null;
  target: MeshTarget | null;
  controls: MachineControls;
  pairInstanceId: string | null;
  lastSeenAt: number | null;
}
export interface SelfRowView {
  readOnly?: boolean;
  planLimited: boolean;
  name: string;
  installationId: string;
  metadataRevision: number;
  displayName: string | null;
}
export type MyMachinesView =
  | { kind: "loading" }
  | { kind: "sign_in"; headline: string; blocked: boolean; saved: MachineRowView[]; notes: string[] }
  | { kind: "account"; machineLimit?: { limit: number; activeCount: number }; reauthRequired: boolean; removed?: boolean; self: SelfRowView | null; machines: MachineRowView[]; saved: MachineRowView[]; notes: string[] };
export interface MyMachinesInput {
  account: AccountPresence;
  snapshot: AccountMeshSnapshot | null;
  listFailure?: AccountMeshFailure | null;
  now: number;
}

export function myMachinesView({ account, snapshot, listFailure = null, now }: MyMachinesInput): MyMachinesView {
  if (account === "pending" || !snapshot) return { kind: "loading" };
  const notes: string[] = [];
  if (snapshot.cleanup === "pending") notes.push("Address cleanup is pending: endpoint withdrawal is unconfirmed.");
  if (snapshot.registry?.cursor) notes.push("Showing part of your account's machines. Refresh to load more.");
  if (account === "signed_out") {
    const blocked = snapshot.traffic === "blocked_signed_out";
    return { kind: "sign_in", headline: blocked ? SIGNED_OUT_COPY : SIGN_IN_COPY, blocked, saved: [], notes };
  }
  const locked = snapshot.traffic === "plan_limit_locked";
  const refusal = snapshot.last_refusal;
  const machineLimit = locked && refusal?.reason === "mesh.machines" ? { limit: refusal.limit, activeCount: refusal.active_count } : undefined;
  const usable = snapshot.traffic !== "blocked_signed_out" && snapshot.binding !== "other" && refusal?.reason !== "enrollment_budget";
  const selfId = snapshot.local_installation_id;
  const unique = new Map((snapshot.registry?.rows ?? []).map(row => [row.installation_id, row]));
  const machines = [...unique.values()].filter(row => row.installation_id !== selfId).map(row => {
    const pair = snapshot.pairs.find(pair => pair.installation_id === row.installation_id) ?? null;
    const result = machineRow(row, pair, now, usable);
    return machineLimit ? { ...result, controls: { ...result.controls, plans: true } } : result;
  });
  if (refusal && !snapshot.publication_failure) notes.push(enrollmentRetryMessage(snapshot, now) ?? failureMessage(refusal, DEV_BUILD, snapshot.retry_at ?? undefined, true));
  if (snapshot.publication_failure) notes.push(`Mesh address: ${failureMessage(snapshot.publication_failure, DEV_BUILD, snapshot.retry_at ?? undefined, true)}`);
  const failure = listFailure ?? snapshot.registry_failure;
  if (failure) notes.push(`${snapshot.registry ? "Showing the last account list. " : ""}${failureMessage(failure)}`);
  const own = snapshot.self;
  const ownRow = own ? unique.get(own.installation_id) : null;
  return {
    kind: "account", machineLimit,
    removed: refusal?.reason === "revoked" || ownRow?.active === false,
    reauthRequired: snapshot.enrollment === "refused" && ["reauth_required", "generation_mismatch", "key_conflict"].includes(refusal?.reason ?? ""),
    self: own ? { readOnly: !usable, planLimited: locked, name: ownRow?.display_name ?? own.display_name ?? "This machine", installationId: own.installation_id,
      metadataRevision: ownRow?.metadata_revision ?? own.metadata_revision, displayName: ownRow?.display_name ?? own.display_name } : null,
    machines, saved: [], notes,
  };
}

function machineRow(row: AccountMachineRow, pair: LocalPairFact | null, now: number, usable: boolean): MachineRowView {
  const name = row.display_name ?? "Unnamed machine";
  const target: MeshTarget = { installation_id: row.installation_id, key_id: row.key_id, key_generation: row.key_generation, slot_revision: row.slot_revision };
  const base = { key: row.installation_id, name, transport: row.endpoint?.transport, online: pair?.online ?? null,
    target: usable ? target : null, pairInstanceId: pair?.instance_id ?? null, lastSeenAt: row.authenticated_at,
    canSignOut: usable && row.presence !== "signed_out", controls: { revoke: usable && row.active ? target : null } };
  if (!row.active || row.eligibility === "revoked") return { ...base, status: "revoked", detail: "Removed from your mesh", online: false };
  if (row.presence === "signed_out") return { ...base, status: "signed_out", detail: `Sign in on ${name}`, online: false };
  if (row.eligibility === "mesh-allowance") return { ...base, status: "over_limit", detail: "Outside your plan's machine limit" };
  if (row.eligibility === "verification-unavailable") return { ...base, status: "unverified", detail: "Account verification is unavailable" };
  if (!usable) return { ...base, status: "blocked", detail: "Mesh is blocked on this machine" };
  if (row.membership_expires_at !== null && row.membership_expires_at <= now) return { ...base, status: "expired", detail: `Membership expired — open Domios on ${name} to renew`, online: false };
  if (pair?.online) return { ...base, status: "linked", detail: "Connected over your account mesh" };
  if (!row.endpoint) return { ...base, status: "endpoint_missing", detail: `Open Domios on ${name} to publish its endpoint`, online: false };
  if (row.endpoint.expires_at <= now) return { ...base, status: "endpoint_stale", detail: "Stale endpoint — this installation may be offline", online: false };
  return { ...base, status: "linking", detail: "Connecting automatically over your account mesh" };
}

export function accountPresence(kind: "pending" | "signed_out" | "signed_in" | "error"): AccountPresence {
  return kind === "error" ? "signed_out" : kind;
}

export function meshFailureToast(operation: string, failure: AccountMeshFailure): Omit<Notification, "timestamp" | "read"> {
  return {
    id: "mesh-my-machines-failure",
    title: failure.reason === "rate-limited" ? "Mesh is busy" : `Could not ${operation}`,
    body: failureMessage(failure),
    severity: failure.reason === "rate-limited" ? "Info" : "Warning",
    level: failure.reason === "rate-limited" ? "verbose" : "action_needed",
    tab_id: null,
    workspace_id: null,
  };
}

export function failureMessage(failure: AccountMeshFailure, devBuild = DEV_BUILD, retryAt?: number, retrying = false): string {
  switch (failure.reason) {
    case "key_conflict": return "This machine has a different installation key from the account registry. Rejoin mesh.";
    case "generation_mismatch": return "This machine's key is out of date. Rejoin mesh.";
    case "challenge_invalid": return "The installation challenge was rejected. Try enrolling this machine again.";
    case "enrollment_budget": return failure.retry_after_seconds != null
      ? `Enrollment limit reached; retry after ${new Date((retryAt ?? Math.floor(Date.now() / 1000) + failure.retry_after_seconds) * 1000).toISOString()}`
      : "Account enrollment limit reached, retrying automatically";
    case "proof_invalid": return "This build cannot join this account's mesh (proof rejected)." + (devBuild ? " DEV builds use the QA target." : "");
    case "mesh.machines": return `Your plan allows ${failure.limit} machines.`;
    case "rate-limited": return `${retrying ? "Busy, retrying at" : "Busy until"} ${new Date((retryAt ?? Math.floor(Date.now() / 1000) + failure.retry_after_seconds) * 1000).toISOString()}`;
    case "offline": return "This machine is offline.";
    case "peer-unreachable": return "That machine could not be reached. Both machines must be online.";
    case "handshake-failed": return "The machines could not complete an authorized handshake.";
    case "signed-out": return SIGNED_OUT_COPY;
    case "revoked": return "This machine was removed from your mesh. Rejoin mesh.";
    case "cancelled": return "The account changed before the request finished. Try again.";
    case "plan_limit_locked": return PLAN_LOCKED_COPY;
    case "reauth_required": return REAUTH_REQUIRED_COPY;
    case "verification-unavailable": return "Machine membership could not be verified right now. Try again later.";
    case "unauthenticated":
    case "app-check": return "The account session could not be verified. Retrying automatically.";
    case "forbidden": return "Your account does not allow this action.";
    case "conflict": return "That machine changed since the list was loaded. Refresh and try again.";
    case "invalid": return "The request was rejected as invalid.";
    case "authority-unavailable":
    case "unavailable": return "The account service is unavailable. Try again later.";
  }
}

export function enrollmentRetryMessage(snapshot: AccountMeshSnapshot, now: number): string | null {
  const refusal = snapshot.last_refusal;
  if (snapshot.enrollment !== "refused" || refusal?.reason !== "enrollment_budget") return null;
  if (snapshot.retry_at == null) return "Account enrollment limit reached. Try again later.";
  if (refusal.retry_after_seconds == null) return failureMessage(refusal);
  const remaining = snapshot.retry_at == null ? refusal.retry_after_seconds : Math.max(0, snapshot.retry_at - now);
  return failureMessage({ ...refusal, retry_after_seconds: remaining }, DEV_BUILD, snapshot.retry_at ?? now + remaining);
}
