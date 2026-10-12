import { AdminError, type Audit, type AuthProvider, type Backend, type Claims, type DataProvider, type Entitlement, type Identity, type User } from "../domain/model";

export interface State { users: User[]; entitlements: Record<string, Entitlement>; audit: Audit[] }
export function seedState(): State {
  return {
    users: [
      { uid: "local-admin", email: "admin@localhost", providers: ["local"], createdAt: "2026-01-01T00:00:00Z", lastSignInAt: null, disabled: false, claims: { admin: true, beta: true } },
      { uid: "local-user", email: "user@localhost", providers: ["local"], createdAt: "2026-01-02T00:00:00Z", lastSignInAt: null, disabled: false, claims: {} },
    ],
    entitlements: {}, audit: [],
  };
}
export class FakeBackend implements Backend, AuthProvider, DataProvider {
  readonly auth = this;
  readonly data = this;
  sessions = new Map<string, Identity>();
  tokens = new Map<string, Identity>();
  constructor(public state: State = seedState()) {}
  async verifySession(cookie: string) {
    const identity = this.sessions.get(cookie);
    if (!identity) throw new Error("Invalid session");
    return structuredClone(identity);
  }
  async verifyIdToken(token: string) {
    const identity = this.tokens.get(token);
    if (!identity) throw new Error("Invalid token");
    return structuredClone(identity);
  }
  async createSession(token: string, expiresIn: number) {
    const identity = await this.verifyIdToken(token);
    const session = `session-${this.sessions.size}`;
    this.sessions.set(session, { ...identity, expiresAt: Date.now() / 1000 + expiresIn / 1000 });
    return session;
  }
  async getUser(uid: string) {
    const user = this.state.users.find(user => user.uid === uid);
    if (!user) throw new AdminError(404, "User not found");
    return structuredClone(user);
  }
  async setClaims(uid: string, claims: Claims) {
    const user = this.state.users.find(user => user.uid === uid);
    if (!user) throw new AdminError(404, "User not found");
    user.claims = structuredClone(claims);
  }
  async findUser(query: string) {
    return structuredClone(this.state.users.find(user => user.uid === query || user.email === query) ?? null);
  }
  async detail(uid: string) {
    const user = await this.findUser(uid);
    return user ? { user, entitlement: structuredClone((Object.hasOwn(this.state.entitlements, uid) ? this.state.entitlements[uid] : null)), customerUrl: null,
      audit: structuredClone(this.state.audit.filter(entry => entry.uid === uid).reverse()) } : null;
  }
  protected apply(uid: string, build: (before: Entitlement | null) => Audit) {
    const entry = build(structuredClone((Object.hasOwn(this.state.entitlements, uid) ? this.state.entitlements[uid] : null)));
    if (this.state.audit.some(existing => existing.id === entry.id)) throw new Error("Audit already exists");
    this.state = { ...this.state, entitlements: { ...this.state.entitlements, [uid]: structuredClone(entry.after) },
      audit: [...this.state.audit, structuredClone(entry)] };
    return structuredClone(entry);
  }
  async change(uid: string, build: (before: Entitlement | null) => Audit) { return this.apply(uid, build); }
}
