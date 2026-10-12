import { AdminError, type AuthProvider, type Claims, type Identity } from "./model";

export const SESSION_COOKIE = "__Host-admin";
export const SESSION_SECONDS = 8 * 60 * 60;
export const COOKIE_OPTIONS = { httpOnly: true, secure: true, sameSite: "lax" as const, path: "/" };

export function mergeAdminClaim(existing: Claims | undefined): Claims {
  return { ...existing, admin: true };
}
export async function grantAdminClaim(auth: AuthProvider, uid: string): Promise<void> {
  const user = await auth.getUser(uid);
  await auth.setClaims(uid, mergeAdminClaim(user.claims));
}
async function checkIdentity(auth: AuthProvider, identity: Identity, now: number): Promise<Identity> {
  if (!identity.uid || identity.claims.admin !== true || (!Number.isFinite(identity.expiresAt) || identity.expiresAt <= now)) {
    throw new AdminError(403, "Admin access required");
  }
  const user = await auth.getUser(identity.uid);
  if (user.disabled || user.claims.admin !== true) throw new AdminError(403, "Admin access required");
  return identity;
}
export async function requireAdmin(auth: AuthProvider, cookie: string | undefined, now = Date.now() / 1000): Promise<Identity> {
  try {
    if (!cookie) throw new Error("Missing session");
    return await checkIdentity(auth, await auth.verifySession(cookie), now);
  } catch {
    throw new AdminError(403, "Admin access required");
  }
}
export async function createAdminSession(auth: AuthProvider, token: string, now = Date.now() / 1000): Promise<string> {
  try {
    const identity = await checkIdentity(auth, await auth.verifyIdToken(token), now);
    if (!Number.isFinite(identity.authenticatedAt) || identity.authenticatedAt > now || now - identity.authenticatedAt > 300) throw new Error("Sign in again");
    return await auth.createSession(token, SESSION_SECONDS * 1000);
  } catch {
    throw new AdminError(403, "A recent admin sign-in is required");
  }
}
