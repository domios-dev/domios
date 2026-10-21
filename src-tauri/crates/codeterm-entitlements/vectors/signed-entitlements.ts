import type { ApplicationEnvelope } from "./application-access";
import type { AuthenticatedAccount, ProducerMode } from "./desktop-provenance";
import type { ConsentMetadataHook, ConsentMetadata } from "./monetization-transition";

export type LimitScope = "installation" | "account";
export interface ScopedLimit { max: number | null; scope: LimitScope }
export interface ScopedLimits {
  "mesh.machines": ScopedLimit & { scope: "account" };
  workspaces: ScopedLimit & { scope: "installation" };
  "mem.items": ScopedLimit & { scope: "installation" };
}
export const COMPILED_FREE_LIMITS: Readonly<ScopedLimits> = Object.freeze({
  "mesh.machines": Object.freeze({max: 2, scope: "account"}),
  workspaces: Object.freeze({max: 2, scope: "installation"}),
  "mem.items": Object.freeze({max: 1000, scope: "installation"}),
});
export interface GrantPayload {
  schema_version: 1;
  iss: "domios-entitlements";
  aud: { project: string; env: ProducerMode };
  sub: string;
  inst: string;
  revision: number;
  policy_version: number;
  policy_epoch: string;
  issued_at: number;
  not_before: number;
  refresh_after: number;
  expires_at: number;
  grace_until: number;
  authority: "admin" | "billing" | "free";
  label: "admin" | "pro" | "sponsor" | "free";
  enforcement: { enabled: boolean; rollback: boolean };
  limits: ScopedLimits;
  /** Reviewed capability names only; unknown names confer nothing. */
  capabilities: readonly string[];
  perks: { sponsor: boolean; priority_support: boolean };
  period_end: number | null;
  /** Optional committed whole-account transition reason; never inferred from cached expiry. */
  transition_reason?: "period_ended" | "cancelled" | "downgraded" | "revoked" | "gift_expired";
}
/** payload is base64url of exact RFC8785 UTF-8 bytes, NOT a parsed JSON object. */
export interface DetachedGrant { payload: string; signature: string; key_id: string; alg: "Ed25519" }
export interface EntitlementsRequest { uid?: string; installation_id?: string }
export type EntitlementsResponse = ApplicationEnvelope & { grant?: DetachedGrant; consent?: ConsentMetadata };
export interface TrustedGrantKey { key_id: string; alg: "Ed25519"; public_key_spki_base64: string }
export type TrustedGrantKeys = readonly [TrustedGrantKey] | readonly [TrustedGrantKey, TrustedGrantKey];
export interface GrantSigner {
  key_id: string;
  alg: "Ed25519";
  sign(jcsUtf8: Uint8Array): Promise<Uint8Array>;
}
export interface GrantPolicy {
  project: string;
  mode: ProducerMode;
  policy_version: number;
  /** Explicit reviewed build/producer input, no default production epoch. */
  policy_epoch: string;
  enforcement: { enabled: boolean; rollback: boolean };
  keys: TrustedGrantKeys;
}
export interface AuthoritySnapshot {
  uid: string;
  mode: ProducerMode;
  /** Digest/fence includes sources, slot/ledger/generation and admission facts. */
  fingerprint: string;
  observed_at: number;
  envelope: ApplicationEnvelope;
}
export interface GrantReservation { revision: number; snapshot: AuthoritySnapshot; reservation_id: string }
export type GrantPublicationFailure = "invalid" | "unavailable" | "authority-changed" | "contention" | "revision-exhausted";
export interface GrantRevisionStore {
  /** Atomic compare+reserve. Gaps allowed; monotonic safe integers, namespaced by mode+uid. */
  reserve(expected: AuthoritySnapshot): Promise<GrantReservation>;
  /** Atomic authority recheck and one publication. Refuses stale snapshots and reused revisions. */
  publish(reservation: GrantReservation, grant: DetachedGrant): Promise<boolean>;
}
export interface EntitlementsService {
  get(account: AuthenticatedAccount, input: EntitlementsRequest): Promise<EntitlementsResponse>;
}
/** Planned factory signature only. No implementation/import/runtime registration in this checkpoint. */
export type EntitlementsServiceFactory = (dependencies: {
  snapshot: (account: AuthenticatedAccount) => Promise<AuthoritySnapshot>;
  revisions: GrantRevisionStore;
  now: () => number;
  policy?: GrantPolicy;
  signer?: GrantSigner;
  consentMetadata?: ConsentMetadataHook;
}) => EntitlementsService;
