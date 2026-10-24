import { projectLinkRequests } from "./accountLinkProjection";
import type { Notification } from "../../lib/notificationTypes";
import type { AccountMachineRow, AccountMeshFailure, AccountMeshSnapshot, LocalPairFact, MeshTarget } from "./accountMeshClient";

export const SIGNED_OUT_COPY = "Signed out — sign in to use the mesh";
export const SIGN_IN_COPY = "Mesh connects your machines through your Domios account.";
export const REAUTH_REQUIRED_COPY = "Sign in again to add this machine to your mesh";

export type AccountPresence = "pending" | "signed_out" | "signed_in";

export type MachineStatus =
  | "signed_out"
  | "approval_requested"
  | "declined"
  | "link_expired"
  | "link_failed"
  | "linked"
  | "linking"
  | "unlinked"
  | "endpoint_missing"
  | "endpoint_stale"
  | "expired"
  | "revoked"
  | "over_limit"
  | "unverified"
  | "legacy"
  | "blocked"
  | "other_account";

export interface MachineControls {
  /** Present when Add is shown; `reason` disables it. */
  add: { reason: string | null } | null;
  revoke: MeshTarget | null;
  unpairInstanceId: string | null;
}

export interface MachineRowView {
  linkRequest?: import("./accountMeshClient").AccountLinkRequest;
  key: string;
  name: string;
  status: MachineStatus;
  detail: string;
  online: boolean | null;
  target: MeshTarget | null;
  controls: MachineControls;
  /** The local pairing behind this row, whether or not this machine may act on it. */
  pairInstanceId: string | null;
  /** Epoch seconds the machine last authenticated to the account. */
  lastSeenAt: number | null;
}

export interface SelfRowView {
  name: string;
  installationId: string;
  metadataRevision: number;
  displayName: string | null;
}

export type MyMachinesView =
  | { kind: "loading" }
  | { kind: "sign_in"; headline: string; blocked: boolean; saved: MachineRowView[]; notes: string[] }
  | { kind: "account"; reauthRequired: boolean; self: SelfRowView | null; machines: MachineRowView[]; saved: MachineRowView[]; notes: string[] };

export interface MyMachinesInput {
  account: AccountPresence;
  snapshot: AccountMeshSnapshot | null;
  listFailure?: AccountMeshFailure | null;
  linking: ReadonlySet<string>;
  /** Epoch seconds. */
  now: number;
}

const NO_CONTROLS: MachineControls = { add: null, revoke: null, unpairInstanceId: null };
export const PLAN_LOCKED_COPY = "Locked — this machine is outside your plan's machine limit";
const SLOT_NOTE = "Signing out blocks Mesh on this machine but keeps its pairings and its account slot.";
const CLEANUP_PENDING_NOTE = "Address cleanup is pending: this machine could not confirm its endpoint was withdrawn.";

export function myMachinesView({ account, snapshot, listFailure = null, linking, now }: MyMachinesInput): MyMachinesView {
  if (account === "pending" || !snapshot) return { kind: "loading" };
  const notes = snapshot.cleanup === "pending" ? [CLEANUP_PENDING_NOTE] : [];
  if (snapshot.registry?.cursor) notes.push("Showing part of your account's machines. Refresh to load more.");
  if (account === "signed_out") {
    const blocked = snapshot.traffic === "blocked_signed_out";
    const saved = snapshot.pairs.map((pair) => blocked ? blockedRow(pair, SIGNED_OUT_COPY) : signedOutLegacyRow(pair));
    return {
      kind: "sign_in",
      headline: blocked ? SIGNED_OUT_COPY : SIGN_IN_COPY,
      blocked,
      saved,
      notes: blocked ? [...notes, SLOT_NOTE] : notes,
    };
  }
  if (snapshot.binding === "other") {
    return {
      kind: "account",
      reauthRequired: snapshot.last_refusal?.reason === "reauth_required",
      self: selfRow(snapshot),
      machines: registryRows(snapshot, [], linking, now, false),
      saved: snapshot.pairs.map((pair) => ({
        ...blockedRow(pair, "Paired under another account — sign in with that account to use it"),
        status: "other_account" as const,
      })),
      notes: [...notes, "Pairings saved on this machine belong to a different account and stay blocked."],
    };
  }
  const blocked = snapshot.traffic === "blocked_signed_out";
  const locked = snapshot.traffic === "plan_limit_locked";
  const pairs = snapshot.pairs;
  const registry = snapshot.registry?.rows ?? [];
  const known = new Set(registry.map((row) => row.installation_id));
  const saved = pairs
    .filter((pair) => !pair.installation_id || !known.has(pair.installation_id))
    .map((pair) => blocked ? blockedRow(pair, "Mesh is blocked until your account finishes resuming") : locked ? lockedRow(pair) : savedPairRow(pair));
  return {
    kind: "account",
    reauthRequired: snapshot.last_refusal?.reason === "reauth_required",
    self: selfRow(snapshot),
    machines: registryRows(snapshot, pairs, linking, now, !blocked && !locked),
    saved,
    notes: [...notes, ...registryNotes(snapshot, listFailure)],
  };
}

function registryNotes(snapshot: AccountMeshSnapshot, listFailure: AccountMeshFailure | null): string[] {
  const notes: string[] = [];
  if (snapshot.enrollment === "refused" && snapshot.last_refusal && !snapshot.publication_failure) {
    notes.push(`Account enrollment was refused. ${failureMessage(snapshot.last_refusal)}`);
  }
  if (snapshot.publication_failure) {
    notes.push(`Mesh address publication was refused. ${failureMessage(snapshot.publication_failure)}`);
  }
  if (snapshot.retry_at != null) {
    notes.push(`Waiting to retry at ${new Date(snapshot.retry_at * 1000).toISOString()}.`);
  }
  const failure = listFailure ?? snapshot.registry_failure;
  if (failure) {
    const message = failureMessage(failure);
    notes.push(snapshot.registry ? `Showing the last account list. ${message}` : `Your account's machines could not be listed. ${message}`);
  }
  return notes;
}

function selfRow(snapshot: AccountMeshSnapshot): SelfRowView | null {
  const self = snapshot.self;
  if (!self) return null;
  const row = snapshot.registry?.rows.find((candidate) => candidate.installation_id === self.installation_id);
  const displayName = row?.display_name ?? self.display_name;
  return {
    name: displayName ?? "This machine",
    installationId: self.installation_id,
    metadataRevision: row?.metadata_revision ?? self.metadata_revision,
    displayName,
  };
}

function registryRows(snapshot: AccountMeshSnapshot, pairs: LocalPairFact[], linking: ReadonlySet<string>, now: number, usable: boolean): MachineRowView[] {
  const selfId = snapshot.self?.installation_id;
  const rows = (snapshot.registry?.rows ?? [])
    .filter((row) => row.installation_id !== selfId)
    .map((row) => machineRow(row, pairs.find((pair) => pair.installation_id === row.installation_id) ?? null, {
      linking: linking.has(row.installation_id),
      now,
      usable,
      enrolled: !!snapshot.self,
    }));
  return usable ? projectLinkRequests(rows, snapshot.link_requests ?? []) : rows;
}

function machineRow(row: AccountMachineRow, pair: LocalPairFact | null, context: { linking: boolean; now: number; usable: boolean; enrolled: boolean }): MachineRowView {
  const name = row.display_name ?? pair?.name ?? "Unnamed machine";
  const base = { key: row.installation_id, name, online: pair ? pair.online : null, target: target(row), pairInstanceId: pair?.instance_id ?? null, lastSeenAt: row.authenticated_at };
  const revoke = context.usable && row.active && row.eligibility !== "revoked" ? target(row) : null;
  const unpairInstanceId = context.usable && pair ? pair.instance_id : null;
  if (!row.active || row.eligibility === "revoked") {
    return { ...base, status: "revoked", detail: pair ? "Revoked from your account — the local pairing stays until you unpair it" : "Revoked from your account", controls: { add: null, revoke: null, unpairInstanceId } };
  }
  if (row.presence === "signed_out") {
    return { ...base, status: "signed_out", detail: `${name} is signed out`, online: false, controls: NO_CONTROLS };
  }
  if (row.eligibility === "mesh-allowance") {
    return { ...base, status: "over_limit", detail: "Outside your plan's machine limit", controls: { add: pair ? null : { reason: "This machine is outside your plan's machine limit" }, revoke, unpairInstanceId } };
  }
  if (row.eligibility === "verification-unavailable") {
    return { ...base, status: "unverified", detail: "Account verification is unavailable", controls: { add: pair ? null : { reason: "Account verification is unavailable right now" }, revoke, unpairInstanceId } };
  }
  if (row.membership_expires_at !== null && row.membership_expires_at <= context.now) {
    const detail = pair ? "Connected · membership expired — traffic resumes when that machine renews" : "Membership expired — that machine needs to come online and renew";
    return { ...base, status: "expired", detail, controls: { add: pair ? null : { reason: "That machine's membership expired" }, revoke, unpairInstanceId } };
  }
  if (pair) {
    return { ...base, status: "linked", detail: pair.online ? "Connected · online" : "Connected · offline", controls: { add: null, revoke, unpairInstanceId } };
  }
  if (context.linking) {
    return { ...base, status: "linking", detail: `Waiting for approval on ${name}`, controls: { add: { reason: "Connecting…" }, revoke: null, unpairInstanceId: null } };
  }
  if (!row.endpoint) {
    return { ...base, status: "endpoint_missing", detail: "Not reachable — open Domios on that machine and sign in", controls: { add: { reason: "That machine has not published a connection endpoint" }, revoke, unpairInstanceId: null } };
  }
  if (row.endpoint.expires_at <= context.now) {
    return { ...base, status: "endpoint_stale", detail: "Last seen endpoint expired — that machine may be offline", controls: { add: { reason: "That machine's endpoint is stale" }, revoke, unpairInstanceId: null } };
  }
  const reason = !context.usable ? "Mesh is blocked on this machine" : !context.enrolled ? "This machine is not enrolled in your account yet" : null;
  return { ...base, status: "unlinked", detail: "Not connected", controls: { add: { reason }, revoke, unpairInstanceId: null } };
}

const pairKey = (pair: LocalPairFact) => pair.installation_id ?? `pair:${pair.instance_id}`;

function savedPairRow(pair: LocalPairFact): MachineRowView {
  const legacy = pair.admission === "legacy";
  return {
    key: pairKey(pair),
    name: pair.name,
    status: legacy ? "legacy" : "linked",
    detail: legacy
      ? `Paired before account Mesh · ${pair.online ? "online" : "offline"}`
      : `Connected · ${pair.online ? "online" : "offline"} · not in your account list`,
    online: pair.online,
    target: null,
    controls: { ...NO_CONTROLS, unpairInstanceId: pair.instance_id },
    pairInstanceId: pair.instance_id,
    lastSeenAt: null,
  };
}

function signedOutLegacyRow(pair: LocalPairFact): MachineRowView {
  if (pair.admission !== "legacy") return blockedRow(pair, "Sign in to use this connection");
  return {
    key: pairKey(pair),
    name: pair.name,
    status: "legacy",
    detail: `Paired before account Mesh · ${pair.online ? "online" : "offline"}`,
    online: pair.online,
    target: null,
    controls: NO_CONTROLS,
    pairInstanceId: pair.instance_id,
    lastSeenAt: null,
  };
}

function lockedRow(pair: LocalPairFact): MachineRowView {
  return { key: pairKey(pair), name: pair.name, status: "over_limit", detail: PLAN_LOCKED_COPY, online: pair.online, target: null, controls: { ...NO_CONTROLS, unpairInstanceId: pair.instance_id }, pairInstanceId: pair.instance_id, lastSeenAt: null };
}

function blockedRow(pair: LocalPairFact, detail: string): MachineRowView {
  return { key: pairKey(pair), name: pair.name, status: "blocked", detail, online: null, target: null, controls: NO_CONTROLS, pairInstanceId: pair.instance_id, lastSeenAt: null };
}

function target(row: MeshTarget): MeshTarget {
  return { installation_id: row.installation_id, key_id: row.key_id, key_generation: row.key_generation, slot_revision: row.slot_revision };
}

export function accountPresence(kind: "pending" | "signed_out" | "signed_in" | "error"): AccountPresence {
  return kind === "error" ? "signed_out" : kind;
}

/** Installation ids whose link is now observed, so their pending Add can settle. */
export function settledLinks(linking: ReadonlySet<string>, snapshot: AccountMeshSnapshot): string[] {
  return [...linking].filter((id) => snapshot.pairs.some((pair) => pair.installation_id === id) || snapshot.link_requests?.some((request) => !request.incoming && request.installation_id === id && request.decision !== "waiting"));
}

export function meshFailureToast(operation: string, failure: AccountMeshFailure): Omit<Notification, "timestamp" | "read"> {
  return {
    id: "mesh-my-machines-failure",
    title: `Could not ${operation}`,
    body: failureMessage(failure),
    severity: "Warning",
    level: "action_needed",
    tab_id: null,
    workspace_id: null,
  };
}

export function failureMessage(failure: AccountMeshFailure): string {
  switch (failure.reason) {
    case "mesh.machines": return `Your plan allows ${failure.limit} machines.`;
    case "rate-limited": return `Too many requests. Try again in ${failure.retry_after_seconds} seconds.`;
    case "declined": return "Declined. Try again to request approval.";
    case "expired": return "Expired. Try again to request approval.";
    case "offline": return "This machine is offline.";
    case "peer-unreachable": return "That machine could not be reached. Both machines must be online.";
    case "handshake-failed": return "The machines could not complete an authorized handshake.";
    case "signed-out": return SIGNED_OUT_COPY;
    case "revoked": return "That machine was removed from your account.";
    case "cancelled": return "The account changed before the request finished. Try again.";
    case "plan_limit_locked": return PLAN_LOCKED_COPY;
    case "reauth_required": return REAUTH_REQUIRED_COPY;
    case "verification-unavailable": return "Machine membership could not be verified right now. Try again later.";
    case "unauthenticated":
    case "app-check": return "Your account session could not be verified. Sign in again.";
    case "forbidden": return "Your account does not allow this action.";
    case "conflict": return "That machine changed since the list was loaded. Refresh and try again.";
    case "invalid": return "The request was rejected as invalid.";
    case "authority-unavailable":
    case "unavailable": return "The account service is unavailable. Try again later.";
  }
}
