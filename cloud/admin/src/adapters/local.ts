import "server-only";
import { createHmac, timingSafeEqual, randomUUID } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { FakeBackend, seedState, type State } from "./fake";
import { AdminError, type Audit, type Entitlement, type Identity } from "../domain/model";

export class LocalBackend extends FakeBackend {
  constructor(private path: string, private secret: string, private password: string) {
    super();
    this.load();
  }
  private load() {
    this.state = existsSync(this.path) ? JSON.parse(readFileSync(this.path, "utf8")) as State : seedState();
  }
  private sign(identity: Identity, kind: string): string {
    const body = Buffer.from(JSON.stringify({ ...identity, kind })).toString("base64url");
    return `${body}.${createHmac("sha256", this.secret).update(body).digest("base64url")}`;
  }
  private verify(token: string, kind: string): Identity {
    const [body, signature, extra] = token.split(".");
    if (!body || !signature || extra) throw new Error("Invalid token");
    const expected = createHmac("sha256", this.secret).update(body).digest();
    const received = Buffer.from(signature, "base64url");
    if (received.length !== expected.length || !timingSafeEqual(received, expected)) throw new Error("Invalid token");
    const value = JSON.parse(Buffer.from(body, "base64url").toString("utf8"));
    if (value.kind !== kind || typeof value.uid !== "string" || typeof value.expiresAt !== "number" ||
      value.expiresAt <= Date.now() / 1000 || typeof value.authenticatedAt !== "number" ||
      !value.claims || typeof value.claims !== "object") throw new Error("Invalid token");
    return value as Identity;
  }
  async login(email: string, password: string) {
    const expected = Buffer.from(this.password);
    const received = Buffer.from(password);
    if (received.length !== expected.length || !timingSafeEqual(expected, received)) throw new AdminError(403, "Sign-in failed");
    this.load();
    const user = await this.findUser(email);
    if (!user || user.disabled) throw new AdminError(403, "Sign-in failed");
    const now = Date.now() / 1000;
    return this.sign({ uid: user.uid, claims: user.claims, expiresAt: now + 300, authenticatedAt: now }, "id");
  }
  async verifySession(cookie: string) { return this.verify(cookie, "session"); }
  async verifyIdToken(token: string) { return this.verify(token, "id"); }
  async createSession(token: string, expiresIn: number) {
    return this.sign({ ...this.verify(token, "id"), expiresAt: Date.now() / 1000 + expiresIn / 1000 }, "session");
  }
  async getUser(uid: string) { this.load(); return super.getUser(uid); }
  async findUser(query: string) { this.load(); return super.findUser(query); }
  async change(uid: string, build: (before: Entitlement | null) => Audit) {
    this.load();
    const before = this.state;
    const entry = this.apply(uid, build);
    const temporary = `${this.path}.${randomUUID()}.tmp`;
    try {
      mkdirSync(dirname(this.path), { recursive: true, mode: 0o700 });
      writeFileSync(temporary, JSON.stringify(this.state), { mode: 0o600 });
      renameSync(temporary, this.path);
    } catch (error) {
      this.state = before;
      throw error;
    }
    return entry;
  }
}
