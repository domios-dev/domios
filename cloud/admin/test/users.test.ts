import { describe, expect, it, vi } from "vitest";
import { FakeBackend } from "../src/adapters/fake";
import { AdminService } from "../src/domain/service";
import { matchesPageFilters, nextPageStack, previousPageStack } from "../src/domain/users";
import type { UserSummary } from "../src/domain/model";

function fixture() {
  const backend = new FakeBackend();
  backend.sessions.set("admin", { uid: "local-admin", claims: { admin: true }, expiresAt: Date.now() / 1000 + 3600, authenticatedAt: Date.now() / 1000 });
  backend.state.users[1].claims = { beta: true };
  return { backend, service: new AdminService(backend, ["pro"]) };
}
const confirmation = { uid: "local-user", email: "user@localhost" };
describe("loaded user pages", () => {
  it.each([25, 50, 100])("returns stable %i-user pages and bounded profile lookups", async size => {
    const { backend, service } = fixture();
    const seed = backend.state.users[1];
    backend.state.users = Array.from({ length: size + 3 }, (_, index) => ({ ...seed, uid: `user-${String(index).padStart(3, "0")}` })).concat(backend.state.users[0]);
    backend.state.profiles = ["user-000", "user-001"];
    const list = vi.spyOn(backend, "listUsers"); const profiles = vi.spyOn(backend, "profiles");
    const first = await service.list("admin", size);
    expect(first.users).toHaveLength(size); expect(first.nextPageToken).toBeDefined();
    expect(list).toHaveBeenCalledTimes(1); expect(profiles.mock.calls[0][0]).toHaveLength(size);
    const second = await service.list("admin", size, first.nextPageToken);
    expect(second.users).toHaveLength(4); expect(second.nextPageToken).toBeUndefined();
    expect(new Set([...first.users, ...second.users].map(user => user.uid)).size).toBe(size + 4);
    expect(await service.list("admin", size)).toEqual(first);
    expect(first.users.filter(user => user.hasProfile).map(user => user.uid)).toEqual(["user-000", "user-001"]);
  });
  it("validates pages and rejects non-admin reads before listing", async () => {
    const { backend, service } = fixture(); const list = vi.spyOn(backend, "listUsers");
    await expect(service.list(undefined, 25)).rejects.toMatchObject({ status: 403 });
    expect(list).not.toHaveBeenCalled();
    await expect(service.list("admin", 26)).rejects.toMatchObject({ status: 400 });
    await expect(service.list("admin", 25, "")).rejects.toMatchObject({ status: 400 });
  });
  it("selects exact loaded-page fixtures for every filter", () => {
    const seed = fixture().backend.state.users[1];
    const users: UserSummary[] = [
      { ...seed, uid: "google", providers: ["google.com"], disabled: true, hasProfile: true },
      { ...seed, uid: "facebook", providers: ["facebook.com"], hasProfile: false },
      { ...seed, uid: "email", providers: ["password"], hasProfile: true },
      { ...seed, uid: "anonymous", providers: [], hasProfile: false },
    ];
    const select = (filters = {}) => users.filter(user => matchesPageFilters(user, { provider: "", disabled: "", profile: "", ...filters })).map(user => user.uid);
    for (const provider of ["google", "facebook", "email", "anonymous"]) expect(select({ provider })).toEqual([provider]);
    expect(select({ disabled: "yes" })).toEqual(["google"]);
    expect(select({ disabled: "no" })).toEqual(["facebook", "email", "anonymous"]);
    expect(select({ profile: "yes" })).toEqual(["google", "email"]);
    expect(select({ profile: "no" })).toEqual(["facebook", "anonymous"]);
    expect(select({ provider: "email", disabled: "yes" })).toEqual([]);
    expect(select()).toHaveLength(4);
  });
  it("retains previous page tokens without advancing at the end", () => {
    const stack = nextPageStack(nextPageStack([], "a"), "b");
    expect(stack).toEqual(["a", "b"]); expect(previousPageStack(stack)).toEqual(["a"]);
    expect(previousPageStack([])).toEqual([]); expect(nextPageStack(stack, undefined)).toBe(stack);
  });
});
describe("account operations", () => {
  it("disable and enable persist state, revoke tokens, and audit once each", async () => {
    const { backend, service } = fixture(); const revoke = vi.spyOn(backend, "revokeTokens");
    const disabled = await service.account("admin", "local-user", "disable", " Support ", confirmation);
    expect((await backend.getUser("local-user")).disabled).toBe(true);
    expect(disabled).toMatchObject({ actor: "local-admin", reason: "Support", before: { disabled: false }, after: { disabled: true } });
    await service.account("admin", "local-user", "enable", "Restored", confirmation);
    expect((await backend.getUser("local-user")).disabled).toBe(false);
    expect(revoke).toHaveBeenCalledTimes(2); expect((await backend.detail("local-user"))?.audit).toHaveLength(2);
  });
  it("grants and revokes admin while preserving other claims and auditing", async () => {
    const { backend, service } = fixture(); const revoke = vi.spyOn(backend, "revokeTokens");
    await service.account("admin", "local-user", "grant-admin", "Approved", confirmation);
    expect((await backend.getUser("local-user")).claims).toEqual({ beta: true, admin: true });
    await service.account("admin", "local-user", "revoke-admin", "Removed", confirmation);
    expect((await backend.getUser("local-user")).claims).toEqual({ beta: true, admin: false });
    expect(backend.state.audit.map(entry => entry.action)).toEqual(["grant-admin", "revoke-admin"]);
    expect(revoke).toHaveBeenCalledTimes(2);
  });
  it.each(["disable", "enable", "revoke-admin"])("refuses self %s", async action => {
    const { backend, service } = fixture();
    await expect(service.account("admin", "local-admin", action, "Reason", { uid: "local-admin", email: "admin@localhost" })).rejects.toMatchObject({ status: 403 });
    expect(backend.state.audit).toEqual([]); expect(backend.state.users[0].disabled).toBe(false);
  });
  it.each(["disable", "revoke-admin"])("refuses last enabled admin %s", async action => {
    const { backend, service } = fixture(); backend.state.users[1].claims.admin = true;
    vi.spyOn(backend, "listUsers").mockResolvedValue({ users: [await backend.getUser("local-user")] });
    await expect(service.account("admin", "local-user", action, "Reason", confirmation)).rejects.toMatchObject({ status: 403 });
    expect(backend.state.audit).toEqual([]); expect(backend.state.users[1].disabled).toBe(false);
  });
  it("finds another admin on a later token page", async () => {
    const { backend, service } = fixture(); backend.state.users[1].claims.admin = true;
    const list = vi.spyOn(backend, "listUsers").mockResolvedValueOnce({ users: [await backend.getUser("local-user")], nextPageToken: "next" })
      .mockResolvedValueOnce({ users: [await backend.getUser("local-admin")] });
    await service.account("admin", "local-user", "revoke-admin", "Reason", confirmation);
    expect(list).toHaveBeenNthCalledWith(2, 1000, "next");
  });
  it.each(["disable", "enable", "grant-admin", "revoke-admin"])("rejects non-admin %s before mutations", async action => {
    const { backend, service } = fixture(); const write = vi.spyOn(backend, "setClaims"); const disable = vi.spyOn(backend, "setDisabled");
    await expect(service.account(undefined, "local-user", action, "Reason", confirmation)).rejects.toMatchObject({ status: 403 });
    expect(write).not.toHaveBeenCalled(); expect(disable).not.toHaveBeenCalled(); expect(backend.state.audit).toEqual([]);
  });
  it("requires a reason and current confirmation target", async () => {
    const { backend, service } = fixture();
    for (const [reason, target] of [["", confirmation], ["Reason", { uid: "another", email: confirmation.email }], ["Reason", { uid: confirmation.uid, email: "old@example.com" }]] as const) {
      await expect(service.account("admin", "local-user", "disable", reason, target)).rejects.toMatchObject({ status: 400 });
    }
    expect(backend.state.audit).toEqual([]);
  });
});
