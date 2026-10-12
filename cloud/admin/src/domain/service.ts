import { randomUUID } from "node:crypto";
import { requireAdmin } from "./auth";
import { AdminError, validUid, type Audit, type Backend } from "./model";

export class AdminService {
  constructor(private backend: Backend, readonly plans: string[]) {}
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
