import { randomUUID } from "node:crypto";
import { requireAdmin } from "./auth";
import { AdminError, validUid, type AccountAction, type Audit, type Backend } from "./model";

export class AdminService {
  constructor(private backend: Backend, readonly plans: string[]) {}
  async list(cookie: string | undefined, pageSize: number, pageToken?: string) {
    await requireAdmin(this.backend.auth, cookie);
    if (![25, 50, 100].includes(pageSize) || (pageToken !== undefined && (!pageToken || pageToken.length > 4096))) {
      throw new AdminError(400, "Invalid page size or token");
    }
    const page = await this.backend.auth.listUsers(pageSize, pageToken);
    const profiles = new Set(await this.backend.data.profiles(page.users.map(user => user.uid)));
    return { users: page.users.map(user => ({ ...user, hasProfile: profiles.has(user.uid) })), nextPageToken: page.nextPageToken };
  }
  async account(cookie: string | undefined, uid: string, action: string, reason: string, confirmation: { uid: string; email: string }) {
    const actor = await requireAdmin(this.backend.auth, cookie);
    if (!validUid(uid) || !["disable", "enable", "grant-admin", "revoke-admin"].includes(action) ||
      !reason.trim() || reason.trim().length > 2000) throw new AdminError(400, "Choose an action and provide a reason (1–2000 characters)");
    const before = await this.backend.auth.getUser(uid);
    if (confirmation.uid !== uid || confirmation.email !== (before.email ?? "")) throw new AdminError(400, "Confirm the current target email and uid");
    if (actor.uid === uid && action !== "grant-admin") throw new AdminError(403, "You cannot change your own account status or revoke your admin role");
    if ((action === "disable" || action === "revoke-admin") && before.claims.admin === true && !before.disabled) {
      let token: string | undefined;
      let otherAdmin = false;
      do {
        const page = await this.backend.auth.listUsers(1000, token);
        otherAdmin = page.users.some(user => user.uid !== uid && !user.disabled && user.claims.admin === true);
        token = page.nextPageToken;
      } while (!otherAdmin && token);
      if (!otherAdmin) throw new AdminError(403, "Cannot remove the last enabled admin");
    }
    if (action === "disable" || action === "enable") await this.backend.auth.setDisabled(uid, action === "disable");
    else await this.backend.auth.setClaims(uid, { ...before.claims, admin: action === "grant-admin" });
    await this.backend.auth.revokeTokens(uid);
    const entry = {
      id: randomUUID(), uid, actor: actor.uid, action: action as AccountAction,
      reason: reason.trim(), created_at: Math.floor(Date.now() / 1000),
      before, after: await this.backend.auth.getUser(uid),
    };
    await this.backend.data.appendAccountAudit(entry);
    return entry;
  }
  async search(cookie: string | undefined, query: string) {
    await requireAdmin(this.backend.auth, cookie);
    if (!query.trim() || query.length > 320) throw new AdminError(400, "Enter an email or uid");
    return this.backend.data.findUser(query.trim());
  }
  async detail(cookie: string | undefined, uid: string) {
    await requireAdmin(this.backend.auth, cookie);
    if (!validUid(uid)) throw new AdminError(400, "Invalid uid");
    return this.backend.data.detail(uid);
  }
  async change(cookie: string | undefined, uid: string, action: string, plan: string, reason: string): Promise<Audit> {
    const actor = await requireAdmin(this.backend.auth, cookie);
    if (!validUid(uid) || (action !== "grant" && action !== "revoke") ||
      (action === "grant" && !this.plans.includes(plan)) || !reason.trim() || reason.trim().length > 2000) {
      throw new AdminError(400, "Choose a plan and provide a reason (1–2000 characters)");
    }
    await this.backend.auth.getUser(uid);
    const id = randomUUID();
    const created_at = Math.floor(Date.now() / 1000);
    return this.backend.data.change(uid, before => ({
      id, uid, actor: actor.uid, action, reason: reason.trim(), created_at, before,
      after: {
        plan: action === "grant" ? plan : before?.plan ?? "none",
        status: action === "grant" ? "active" : "revoked",
        current_period_end: action === "grant" ? 0 : before?.current_period_end ?? 0,
        cancel_at_period_end: false,
        source_event_id: id, source: "admin", updated_at: created_at,
      },
    }));
  }
}
