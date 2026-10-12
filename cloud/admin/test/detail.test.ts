import { beforeEach, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { FakeBackend } from "../src/adapters/fake";

const injected = vi.hoisted(() => ({ backend: null as unknown, cookie: "admin" as string | undefined }));
vi.mock("../src/adapters", () => ({ getBackend: () => injected.backend }));
vi.mock("next/headers", () => ({ cookies: async () => ({ get: () => injected.cookie ? { value: injected.cookie } : undefined }) }));
vi.mock("next/navigation", () => ({ forbidden: () => { throw new Error("FORBIDDEN_403"); }, notFound: () => { throw new Error("NOT_FOUND"); } }));
import UserPage from "../app/users/[uid]/page";
let backend: FakeBackend;
beforeEach(() => {
  backend = new FakeBackend(); injected.backend = backend; injected.cookie = "admin";
  backend.sessions.set("admin", { uid: "local-admin", claims: { admin: true }, expiresAt: Date.now() / 1000 + 3600, authenticatedAt: Date.now() / 1000 });
});
it("renders persisted diagnostics and named target in the inline confirmation", async () => {
  backend.state.users[1].claims = { beta: "diagnostic-claim" };
  backend.state.entitlements["local-user"] = { plan: "pro", status: "active", source: "admin", source_event_id: "grant-fixture", current_period_end: 0, cancel_at_period_end: false, updated_at: 1 };
  const markup = renderToStaticMarkup(await UserPage({ params: Promise.resolve({ uid: "local-user" }), searchParams: Promise.resolve({ action: "disable" }) }));
  expect(markup).toContain("user@localhost"); expect(markup).toContain("local-user");
  expect(markup).toContain("diagnostic-claim"); expect(markup).toContain("grant-fixture");
  expect(markup).toContain('name="confirmUid" value="local-user"');
  expect(markup).toContain('name="confirmEmail" value="user@localhost"');
  expect(markup).toMatch(/<button(?=[^>]*name="confirmed")(?=[^>]*value="yes")[^>]*>/);
  expect(markup.match(/<details[^>]*>/g)).toEqual(["<details>", "<details>"]);
  expect(markup).not.toContain('name="claims"');
});
it("denies detail rendering for a non-admin session", async () => {
  injected.cookie = undefined;
  await expect(UserPage({ params: Promise.resolve({ uid: "local-user" }), searchParams: Promise.resolve({}) })).rejects.toThrow("FORBIDDEN_403");
});
