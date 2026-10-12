import { beforeEach, describe, expect, it, vi } from "vitest";
import { FakeBackend } from "../src/adapters/fake";
import { SESSION_COOKIE } from "../src/domain/auth";

const injected = vi.hoisted(() => ({ backend: null as unknown, cookie: "admin" as string | undefined, set: vi.fn() }));
vi.mock("../src/adapters", () => ({ getBackend: () => injected.backend }));
vi.mock("next/headers", () => ({ cookies: async () => ({
  get: (name: string) => name === "__Host-admin" && injected.cookie ? { value: injected.cookie } : undefined,
  set: injected.set,
}) }));
vi.mock("next/cache", () => ({ revalidatePath: vi.fn() }));
vi.mock("next/navigation", () => ({
  forbidden: () => { throw new Error("FORBIDDEN_403"); },
  redirect: (path: string) => { throw new Error(`REDIRECT:${path}`); },
}));
import { changeEntitlement, signOut } from "../app/users/actions";

let backend: FakeBackend;
beforeEach(() => {
  backend = new FakeBackend(); injected.backend = backend; injected.cookie = "admin"; injected.set.mockClear();
  backend.sessions.set("admin", { uid: "local-admin", claims: { admin: true }, expiresAt: Date.now() / 1000 + 3600, authenticatedAt: Date.now() / 1000 });
});
function form(action: string, reason: string) {
  const data = new FormData();
  for (const [key, value] of Object.entries({ uid: "local-user", action, reason, plan: "pro", actor: "forged-actor" })) data.set(key, value);
  return data;
}
describe("server actions", () => {
  it("redirects only after grant and revoke have persisted the audit and entitlement", async () => {
    await expect(changeEntitlement(form("grant", "Reviewed request"))).rejects.toThrow("REDIRECT:/users/local-user?saved=1");
    expect(backend.state.entitlements["local-user"]).toMatchObject({ status: "active", source: "admin" });
    expect(backend.state.audit).toHaveLength(1);
    expect(backend.state.audit[0]).toMatchObject({ actor: "local-admin", reason: "Reviewed request", before: null });
    await expect(changeEntitlement(form("revoke", "Reviewed refund"))).rejects.toThrow("REDIRECT:/users/local-user?saved=1");
    expect(backend.state.entitlements["local-user"].status).toBe("revoked");
    expect(backend.state.audit[1].before).toEqual(backend.state.audit[0].after);
    expect(backend.state.audit[1].after).toEqual(backend.state.entitlements["local-user"]);
  });
  it("denies a captured action without the admin cookie", async () => {
    injected.cookie = undefined;
    await expect(changeEntitlement(form("grant", "Replay"))).rejects.toThrow("FORBIDDEN_403");
    expect(backend.state.entitlements).toEqual({}); expect(backend.state.audit).toEqual([]);
  });
  it("clears the secure cookie only for an authorized sign-out", async () => {
    await expect(signOut()).rejects.toThrow("REDIRECT:/signin");
    expect(injected.set).toHaveBeenCalledWith(SESSION_COOKIE, "", expect.objectContaining({ secure: true, httpOnly: true, maxAge: 0 }));
    injected.set.mockClear(); injected.cookie = undefined;
    await expect(signOut()).rejects.toThrow("FORBIDDEN_403");
    expect(injected.set).not.toHaveBeenCalled();
  });
});
