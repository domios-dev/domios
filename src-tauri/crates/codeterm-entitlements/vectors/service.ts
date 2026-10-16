import type { EntitlementsServiceFactory, EntitlementsRequest, GrantPayload, AuthoritySnapshot } from "../../../shared/signed-entitlements";
import { COMPILED_FREE_LIMITS } from "../../../shared/signed-entitlements";
import { GrantError, scalarText, signGrant } from "./signing";
import { sameAuthority,createFirestoreGrantAuthority,type EntitlementAdmission } from "./store";
import { seconds, validId } from "../../../shared/entitlements";
import type { Firestore } from "firebase-admin/firestore";
import type { GrantPolicy,GrantSigner,EntitlementsResponse } from "../../../shared/signed-entitlements";
import type { ConsentMetadataHook } from "../../../shared/monetization-transition";
import type { IntervalCapability } from "../billing/interval-store";

export function parseEntitlementsRequest(value: EntitlementsRequest, uid: string): EntitlementsRequest {
  if (!value || typeof value !== "object" || Array.isArray(value) || Object.keys(value).some(key => !["uid", "installation_id"].includes(key))
    || (Object.hasOwn(value, "uid") && value.uid !== uid)) throw new GrantError("invalid");
  if (Object.hasOwn(value, "installation_id") && (!scalarText(value.installation_id, 128)
    || /[\/\\]/u.test(value.installation_id) || [".", ".."].includes(value.installation_id))) throw new GrantError("invalid");
  return value;
}
function payload(snapshot: AuthoritySnapshot, revision: number, installation: string, policy: NonNullable<Parameters<EntitlementsServiceFactory>[0]["policy"]>, now: number): GrantPayload {
  const {envelope} = snapshot, access = envelope.application_access;
  if (envelope.schema_version !== 2 || access.schema_version !== 1 || snapshot.mode !== policy.mode || !seconds(now)) throw new GrantError("invalid");
  const admin = access.authority === "admin";
  if (!admin && (envelope.billing_inventory !== "verified" || !envelope.billing)) throw new GrantError("unavailable");
  const authority = admin ? "admin" : envelope.billing!.plan === "free" ? "free" : "billing";
  if (access.authority !== authority) throw new GrantError("invalid");
  const period_end = authority === "billing" ? envelope.billing!.period_end : null;
  const expires_at = admin ? now + 86400 : Math.min(now + 604800, period_end === null ? Infinity : period_end + 604800);
  return {schema_version: 1, iss: "domios-entitlements", aud: {project: policy.project, env: policy.mode}, sub: snapshot.uid,
    inst: installation, revision, policy_version: policy.policy_version, policy_epoch: policy.policy_epoch,
    issued_at: now, not_before: now, refresh_after: now + 86400, expires_at,
    grace_until: admin ? now + 259200 : Math.min(expires_at + 1209600, period_end === null ? Infinity : period_end + 604800),
    authority, label: admin ? "admin" : envelope.billing!.plan, enforcement: {...policy.enforcement},
    limits: authority === "free" ? structuredClone(COMPILED_FREE_LIMITS) : {
      "mesh.machines": {max: access.max_mesh_machines, scope: "account"},
      workspaces: {max: access.max_workspaces, scope: "installation"}, "mem.items": {max: null, scope: "installation"}},
    capabilities: [], perks: {sponsor: authority === "free" ? false : access.sponsor,
      priority_support: authority === "free" ? false : access.priority_support}, period_end};
}
export const createEntitlementsService: EntitlementsServiceFactory = dependencies => ({
  async get(account, input) {
    if (!account || !validId(account.uid) || !account.project_id || !["test", "live"].includes(account.mode)
      || !["google.com", "github.com", "apple.com", "microsoft.com", "facebook.com", "password"].includes(account.provider)
      || !seconds(account.authenticated_at) || !seconds(account.checked_at)) throw new GrantError("invalid");
    const parsed = parseEntitlementsRequest(input, account.uid);
    let snapshot = await dependencies.snapshot(account);
    const owned = (value: AuthoritySnapshot) => {
      if (value.uid !== account.uid || value.mode !== account.mode || !scalarText(value.fingerprint, 256)) throw new GrantError("invalid");
    };
    owned(snapshot);
    const metadata = async () => {
      try {
        const consent = await dependencies.consentMetadata?.(account);
        return consent && ["on", "off"].includes(consent.state) && Number.isSafeInteger(consent.revision) && consent.revision >= 0
          ? {consent: {state: consent.state, revision: consent.revision}} : {};
      } catch { return {}; }
    };
    if (!parsed.installation_id || !dependencies.signer || !dependencies.policy) return {...snapshot.envelope, ...await metadata()};
    const policy = dependencies.policy;
    if (policy.project !== account.project_id || policy.mode !== account.mode) throw new GrantError("invalid");
    // Fixed bound, never a polling loop. Each iteration reacquires authoritative admission/snapshot.
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        const reservation = await dependencies.revisions.reserve(snapshot);
        const grant = await signGrant(payload(snapshot, reservation.revision, parsed.installation_id, policy, dependencies.now()),
          policy, dependencies.signer, dependencies.now());
        const current = await dependencies.snapshot(account); owned(current);
        const expires = JSON.parse(Buffer.from(grant.payload, "base64url").toString("utf8")).expires_at as number;
        if (expires > dependencies.now() && sameAuthority(snapshot, current) && await dependencies.revisions.publish(reservation, grant)) {
          return {...snapshot.envelope, grant, ...await metadata()};
        }
        snapshot = current;
      } catch (error) {
        if (!(error instanceof GrantError) || error.code !== "authority-changed") throw error;
        snapshot = await dependencies.snapshot(account); owned(snapshot);
      }
    }
    throw new GrantError("contention");
  },
});

/** Request-scoped composition; no default key, signer, policy epoch or runtime config lookup.
 * authenticate rechecks current token/user OUTSIDE every retrying DB callback. When it cannot
 * provide reviewed linked identity, preserve schema2 without an unbound/anonymous signed grant.
 */
export function createFirestoreEntitlementsHandler(dependencies:{db:Firestore;project:string;mode:"test"|"live";
  authenticate:()=>Promise<EntitlementAdmission>;now:()=>number;emulator?:boolean;
  policy?:GrantPolicy;signer?:GrantSigner;consentMetadata?:ConsentMetadataHook;intervals?:IntervalCapability}) {
  return async (input:EntitlementsRequest):Promise<EntitlementsResponse> => {
    let admission=await dependencies.authenticate();
    const uid=admission.identity.uid;
    const parsed=parseEntitlementsRequest(input,uid);
    const authority=createFirestoreGrantAuthority({...dependencies,admission:()=>admission});
    const account=admission.linked_account;
    if (!account) return (await authority.snapshot(uid)).envelope;
    if (account.uid !== uid || account.project_id !== dependencies.project || account.mode !== dependencies.mode) throw new GrantError("invalid");
    const assertLinked=() => {
      const {identity,current,linked_account:linked}=admission;
      if (!linked || !current.providers.includes(linked.provider)) throw new GrantError("unavailable");
      if (identity.provider === linked.provider) return;
      const desktop=identity.claims.domios_desktop as Record<string,unknown>|undefined;
      if (identity.provider !== "custom" || !desktop || desktop.verification !== "firebase-admin" || desktop.schema_version !== 1
        || desktop.uid !== identity.uid || desktop.project_id !== dependencies.project || desktop.provider !== linked.provider
        || desktop.email !== current.email || desktop.email_verified !== current.emailVerified
        || !seconds(desktop.auth_time) || desktop.auth_time > identity.authenticatedAt || desktop.auth_time < admission.tokens_valid_after) throw new GrantError("unavailable");
    };
    assertLinked();
    return createEntitlementsService({now:dependencies.now,policy:dependencies.policy,signer:dependencies.signer,
      consentMetadata:dependencies.consentMetadata,revisions:authority.revisions,snapshot:async expected => {
        admission=await dependencies.authenticate();
        const refreshed=admission.linked_account;
        if (!refreshed || refreshed.uid !== expected.uid || refreshed.project_id !== expected.project_id
          || refreshed.mode !== expected.mode || refreshed.provider !== expected.provider) throw new GrantError("unavailable");
        assertLinked();
        return authority.snapshot(expected.uid);
      }}).get(account,parsed);
  };
}
