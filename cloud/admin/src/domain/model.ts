export type Claims = Record<string, unknown>;
export interface Identity { uid: string; claims: Claims; expiresAt: number; authenticatedAt: number }
export interface User {
  uid: string;
  email: string | null;
  providers: string[];
  createdAt: string;
  lastSignInAt: string | null;
  disabled: boolean;
  claims: Claims;
}
export interface Entitlement {
  plan: string;
  status: string;
  current_period_end: number;
  cancel_at_period_end: boolean;
  source_event_id: string;
  updated_at: number;
  source?: string;
}
export interface Audit {
  id: string;
  uid: string;
  actor: string;
  reason: string;
  action: "grant" | "revoke";
  created_at: number;
  before: Entitlement | null;
  after: Entitlement;
}
export interface UserDetail { user: User; entitlement: Entitlement | null; customerUrl: string | null; audit: Audit[] }
export interface AuthProvider {
  verifySession(cookie: string): Promise<Identity>;
  verifyIdToken(token: string): Promise<Identity>;
  createSession(token: string, expiresIn: number): Promise<string>;
  getUser(uid: string): Promise<User>;
  setClaims(uid: string, claims: Claims): Promise<void>;
}
export interface DataProvider {
  findUser(query: string): Promise<User | null>;
  detail(uid: string): Promise<UserDetail | null>;
  change(uid: string, build: (before: Entitlement | null) => Audit): Promise<Audit>;
}
export interface Backend { auth: AuthProvider; data: DataProvider }
export class AdminError extends Error {
  constructor(readonly status: 400 | 403 | 404, message: string) { super(message); }
}
export function validUid(uid: string): boolean {
  return uid.length > 0 && uid.length <= 128 && !uid.includes("/") && uid !== "." && uid !== "..";
}
