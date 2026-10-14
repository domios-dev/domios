import { describe, expect, it, vi } from "vitest";
import { FakeBackend } from "../src/adapters/fake";
import { createAdminSession, grantAdminClaim, requireAdmin } from "../src/domain/auth";
import { AdminService } from "../src/domain/service";

function fixture() {
  const backend = new FakeBackend();
  const identity = { uid: "local-admin", claims: { admin: true }, expiresAt: Date.now() / 1000 + 3600, authenticatedAt: Date.now() / 1000 };
  backend.sessions.set("admin", identity);
  backend.tokens.set("id", identity);
  return { backend, identity, service: new AdminService(backend, ["pro"]) };
}
describe("verified admin authorization", () => {
  it.each([undefined, "", "forged"])("rejects missing or invalid session %s with 403", async cookie => {
    await expect(requireAdmin(fixture().backend, cookie)).rejects.toMatchObject({ status: 403 });
  });
  it("rejects expired sessions", async () => {
    const { backend, identity } = fixture();
    backend.sessions.set("expired", { ...identity, expiresAt: 1 });
    await expect(requireAdmin(backend, "expired")).rejects.toMatchObject({ status: 403 });
  });
  it.each([false, "true", 1, undefined])("requires boolean admin claim %s", async admin => {
    const { backend, identity } = fixture();
    backend.sessions.set("non-admin", { ...identity, claims: { admin } });
    await expect(requireAdmin(backend, "non-admin")).rejects.toMatchObject({ status: 403 });
  });
  it("rejects revoked sessions from the verifier", async () => {
    const { backend } = fixture();
    vi.spyOn(backend, "verifySession").mockRejectedValue(new Error("Revoked"));
    await expect(requireAdmin(backend, "admin")).rejects.toMatchObject({ status: 403 });
  });
  it("rechecks current claims, disabled and deleted users", async () => {
    const { backend } = fixture();
    backend.state.users[0].claims = {};
    await expect(requireAdmin(backend, "admin")).rejects.toMatchObject({ status: 403 });
    backend.state.users[0].claims = { admin: true };
    backend.state.users[0].disabled = true;
    await expect(requireAdmin(backend, "admin")).rejects.toMatchObject({ status: 403 });
    backend.state.users = [];
    await expect(requireAdmin(backend, "admin")).rejects.toMatchObject({ status: 403 });
  });
  it("accepts a verified administrator", async () => {
    const { backend } = fixture();
    await expect(requireAdmin(backend, "admin")).resolves.toMatchObject({ uid: "local-admin" });
  });
  it("merges claims in the actual bootstrap operation", async () => {
    const { backend } = fixture();
    backend.state.users[1].claims = { beta: true, roles: ["tester"], admin: false };
    await grantAdminClaim(backend, "local-user");
    expect((await backend.getUser("local-user")).claims).toEqual({ beta: true, roles: ["tester"], admin: true });
  });
  it("requires recent sign-in for session creation", async () => {
    const { backend, identity } = fixture();
    backend.tokens.set("old", { ...identity, authenticatedAt: Date.now() / 1000 - 301 });
    await expect(createAdminSession(backend, "old")).rejects.toMatchObject({ status: 403 });
    const cookie = await createAdminSession(backend, "id");
    await expect(requireAdmin(backend, cookie)).resolves.toMatchObject({ uid: "local-admin" });
  });
});
describe("admin operations", () => {
  it("denies every data operation before touching data", async () => {
    const { backend, service, identity } = fixture();
    backend.sessions.set("user", { ...identity, uid: "local-user", claims: {} });
    const read = vi.spyOn(backend, "findUser");
    const write = vi.spyOn(backend, "change");
    await expect(service.search("user", "local-user")).rejects.toMatchObject({ status: 403 });
    await expect(service.detail("user", "local-user")).rejects.toMatchObject({ status: 403 });
    await expect(service.change("user", "local-user", "grant", "pro", "reason")).rejects.toMatchObject({ status: 403 });
    expect(read).not.toHaveBeenCalled(); expect(write).not.toHaveBeenCalled();
  });
  it("grants and revokes with immutable before and after records", async () => {
    const { backend, service } = fixture();
    const grant = await service.change("admin", "local-user", "grant", "pro", "  Support approval  ");
    const revoke = await service.change("admin", "local-user", "revoke", "", "Refund");
    expect(grant).toMatchObject({ actor: "local-admin", reason: "Support approval", before: null, after: { source: "admin", plan: "pro", status: "active" } });
    expect(revoke.before).toEqual(grant.after);
    expect(revoke.after.status).toBe("revoked");
    const persisted = await backend.detail("local-user");
    expect(persisted?.entitlement).toEqual(revoke.after);
    expect(persisted?.audit).toEqual([revoke, grant]);
  });
  it.each([["grant", "unknown", "reason"], ["grant", "pro", "   "], ["delete", "pro", "reason"]])("rejects invalid change %s %s", async (action, plan, reason) => {
    const { backend, service } = fixture();
    await expect(service.change("admin", "local-user", action, plan, reason)).rejects.toMatchObject({ status: 400 });
    expect(backend.state.audit).toEqual([]);
  });
  it("rejects invalid or missing users without writes", async () => {
    const { backend, service } = fixture();
    await expect(service.change("admin", "users/nested", "grant", "pro", "reason")).rejects.toMatchObject({ status: 400 });
    await expect(service.change("admin", "absent", "grant", "pro", "reason")).rejects.toMatchObject({ status: 404 });
    expect(backend.state.audit).toEqual([]);
  });
});
