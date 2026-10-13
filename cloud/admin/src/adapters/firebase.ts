import { applicationDefault, cert, getApps, initializeApp } from "firebase-admin/app";
import { getAuth, type Auth, type DecodedIdToken, type UserRecord } from "firebase-admin/auth";
import { getFirestore, type Firestore } from "firebase-admin/firestore";
import { AdminError, type AccountAudit, type Audit, type AuthProvider, type Backend, type Claims, type DataProvider, type Entitlement, type User, type AuditEntry } from "../domain/model";

function user(record: UserRecord): User {
  return {
    uid: record.uid, email: record.email ?? null, providers: record.providerData.map(p => p.providerId),
    createdAt: record.metadata.creationTime, lastSignInAt: record.metadata.lastSignInTime ?? null,
    disabled: record.disabled, claims: record.customClaims ?? {},
  };
}
function identity(token: DecodedIdToken) {
  return { uid: token.uid, claims: token, expiresAt: token.exp, authenticatedAt: token.auth_time };
}
export class FirebaseAuthProvider implements AuthProvider {
  constructor(private auth: Auth) {}
  async verifySession(cookie: string) { return identity(await this.auth.verifySessionCookie(cookie, true)); }
  async verifyIdToken(token: string) { return identity(await this.auth.verifyIdToken(token, true)); }
  createSession(token: string, expiresIn: number) { return this.auth.createSessionCookie(token, { expiresIn }); }
  async getUser(uid: string) {
    try { return user(await this.auth.getUser(uid)); }
    catch (error) {
      if (error instanceof Error && "code" in error && error.code === "auth/user-not-found") throw new AdminError(404, "User not found");
      throw error;
    }
  }
  async listUsers(pageSize: number, pageToken?: string) {
    const page = await this.auth.listUsers(pageSize, pageToken);
    return { users: page.users.map(user), nextPageToken: page.pageToken };
  }
  async setDisabled(uid: string, disabled: boolean) { await this.auth.updateUser(uid, { disabled }); }
  revokeTokens(uid: string) { return this.auth.revokeRefreshTokens(uid); }
  setClaims(uid: string, claims: Claims) { return this.auth.setCustomUserClaims(uid, claims); }
}
export class FirebaseDataProvider implements DataProvider {
  constructor(private auth: Auth, private db: Firestore) {}
  async profiles(uids: string[]) {
    if (!uids.length) return [];
    const profiles = await this.db.getAll(...uids.map(uid => this.db.doc(`users/${uid}`)));
    return profiles.filter(profile => profile.exists).map(profile => profile.id);
  }
  async appendAccountAudit(entry: AccountAudit) {
    await this.db.doc(`adminAudit/${entry.id}`).create(entry);
  }
  async findUser(query: string) {
    try { return user(await this.auth.getUser(query)); }
    catch (error) {
      if (!(error instanceof Error && "code" in error && error.code === "auth/user-not-found")) throw error;
      if (!query.includes("@")) return null;
    }
    try { return user(await this.auth.getUserByEmail(query)); }
    catch (error) {
      if (error instanceof Error && "code" in error && error.code === "auth/user-not-found") return null;
      throw error;
    }
  }
  async detail(uid: string) {
    const record = await this.findUser(uid);
    if (!record) return null;
    const [entitlement, audit] = await Promise.all([
      this.db.doc(`entitlements/${uid}`).get(),
      this.db.collection("adminAudit").where("uid", "==", uid).get(),
    ]);
    return {
      user: record, entitlement: (entitlement.data() as Entitlement | undefined) ?? null,
      customerUrl: null,
      audit: audit.docs.map(doc => doc.data() as AuditEntry).sort((a, b) => b.created_at - a.created_at),
    };
  }
  async change(uid: string, build: (before: Entitlement | null) => Audit) {
    return this.db.runTransaction(async transaction => {
      const entitlement = this.db.doc(`entitlements/${uid}`);
      const before = (await transaction.get(entitlement)).data() as Entitlement | undefined;
      const entry = build(before ?? null);
      transaction.create(this.db.doc(`adminAudit/${entry.id}`), entry);
      transaction.set(entitlement, entry.after);
      return entry;
    });
  }
}
export function firebaseBackend(): Backend {
  const existing = getApps().find(app => app.name === "admin-console");
  const app = existing ?? initializeApp({
    projectId: process.env.ADMIN_PROJECT_ID,
    credential: process.env.ADMIN_SERVICE_ACCOUNT
      ? cert(JSON.parse(process.env.ADMIN_SERVICE_ACCOUNT)) : applicationDefault(),
  }, "admin-console");
  const auth = getAuth(app);
  return { auth: new FirebaseAuthProvider(auth), data: new FirebaseDataProvider(auth, getFirestore(app)) };
}
