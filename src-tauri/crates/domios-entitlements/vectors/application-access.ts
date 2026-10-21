import { assertInventory, effective,seconds, type Effective, type EntitlementSource } from "./entitlements";
import { activePrivilege,approvedPrincipal,privilegeMatches,type Privilege,type AdminIdentity,type AdminPrincipal } from "./admin-privilege";
import type { DesktopIdentityEvidence } from "./desktop-provenance";
/** Application-only custom-session admission. Human console/provision/revoke still call
 * activePrivilege directly and never accept this dedicated desktop provenance path.
 * Auth/current-user verification stays outside retrying Firestore callbacks; ledger/slot
 * must come from that callback's current snapshot. No Auth-to-DB atomicity is claimed.
 */
export function applicationPrivilege(value:Privilege|undefined,project:string,identity:AdminIdentity,user:AdminPrincipal,
  slot:unknown,now:number,tokensValidAfter:number,emulator=false):boolean {
  if (!slot || typeof slot !== "object") return false;
  const reservation=slot as Record<string,unknown>;
  if (reservation.phase !== "active" || reservation.uid !== user.uid || reservation.google_subject !== user.googleSubject
    || reservation.generation !== value?.generation) return false;
  if (identity.provider === "google.com") return activePrivilege(value,project,identity,user,now,emulator);
  const raw=identity.claims.domios_desktop;
  if (identity.provider !== "custom" || !raw || typeof raw !== "object" || Array.isArray(raw)
    || Object.keys(raw).sort().join(",") !== "auth_time,email,email_verified,project_id,provider,provider_subject,schema_version,uid,verification") return false;
  const e=raw as DesktopIdentityEvidence;
  return seconds(now) && seconds(tokensValidAfter) && approvedPrincipal(project,user,emulator)
    && identity.uid === user.uid && identity.projectId === project && identity.email === user.email && identity.emailVerified === true
    && seconds(identity.expiresAt) && identity.expiresAt > now && seconds(identity.authenticatedAt) && identity.authenticatedAt <= now
    && e.schema_version === 1 && e.verification === "firebase-admin" && e.provider === "google.com"
    && e.uid === user.uid && e.project_id === project && e.provider_subject === user.googleSubject
    && e.email === user.email && e.email_verified === true && seconds(e.auth_time)
    && e.auth_time <= identity.authenticatedAt && e.auth_time >= tokensValidAfter
    && privilegeMatches(value,project,user) && value.phase === "active"
    && identity.claims.admin === true && user.claims.admin === true
    && identity.claims.admin_generation === value.generation && user.claims.admin_generation === value.generation;
}
export interface ApplicationAccess {
  schema_version:1; authority:"admin"|"billing"|"free"; full_access:boolean;
  max_mesh_machines:number|null; max_workspaces:number|null; priority_support:boolean; sponsor:boolean; admin_generation?:number;
}
export interface ApplicationEnvelope { schema_version:2; billing:Effective|null; billing_inventory:"verified"|"requires_inventory"; application_access:ApplicationAccess }
export function applicationEnvelope(cache:unknown,sources:Record<string,EntitlementSource>,now:number,adminGeneration?:number):ApplicationEnvelope {
  return applicationEnvelopeWithView(cache,sources,now,adminGeneration);
}
// Internal composition only: raw integrity is checked BEFORE evaluating any transient view.
export function applicationEnvelopeWithView(cache:unknown,sources:Record<string,EntitlementSource>,now:number,adminGeneration?:number,
  view?:()=>Record<string,EntitlementSource>):ApplicationEnvelope {
  if(adminGeneration!==undefined && (!Number.isSafeInteger(adminGeneration) || adminGeneration<1))throw new Error("Invalid admin projection");
  let billing:Effective|null=null;
  try { assertInventory(cache,sources);billing=effective(Object.values(view?view():sources),now); }
  catch(error) { if(adminGeneration===undefined)throw error; }
  const admin=adminGeneration!==undefined;
  const ordinary=billing??effective([],now);
  return {schema_version:2,billing,billing_inventory:billing?"verified":"requires_inventory",application_access:{schema_version:1,
    authority:admin?"admin":ordinary.plan==="free"?"free":"billing",full_access:admin,
    max_mesh_machines:admin?null:ordinary.capabilities.max_mesh_machines,max_workspaces:admin?null:ordinary.capabilities.max_workspaces,
    priority_support:admin||ordinary.capabilities.priority_support,sponsor:ordinary.capabilities.sponsor,
    ...(admin?{admin_generation:adminGeneration}:{})}};
}
