import { beforeEach, describe, expect, it, vi } from "vitest";
import { NextRequest } from "next/server";
import { FakeBackend } from "../src/adapters/fake";
import { SESSION_COOKIE } from "../src/domain/auth";

const injected = vi.hoisted(() => ({ backend: null as unknown }));
vi.mock("../src/adapters", () => ({ getBackend: () => injected.backend, providerName: () => "firebase" }));
import { GET } from "../app/api/users/route";
import { POST, DELETE } from "../app/api/session/route";

let backend: FakeBackend;
beforeEach(() => {
  backend = new FakeBackend(); injected.backend = backend;
  backend.tokens.set("id", { uid: "local-admin", claims: { admin: true }, expiresAt: Date.now() / 1000 + 3600, authenticatedAt: Date.now() / 1000 });
});
describe("request boundary", () => {
  it("returns 403 without a claim, with no data leaked", async () => {
    backend.sessions.set("user", { uid: "local-user", claims: {}, expiresAt: Date.now() / 1000 + 3600, authenticatedAt: Date.now() / 1000 });
    const response = await GET(new NextRequest("https://admin.example/api/users?q=local-user", { headers: { cookie: `${SESSION_COOKIE}=user` } }));
    expect(response.status).toBe(403);
    expect(await response.json()).toEqual({ error: "Admin access required" });
  });
  it("returns 403 without a cookie", async () => {
    expect((await GET(new NextRequest("https://admin.example/api/users?q=local-user"))).status).toBe(403);
  });
  it("returns 403 for list reads and invalid pagination is rejected", async () => {
    expect((await GET(new NextRequest("https://admin.example/api/users?pageSize=25"))).status).toBe(403);
    backend.sessions.set("admin", backend.tokens.get("id")!);
    const list = await GET(new NextRequest("https://admin.example/api/users?pageSize=25", { headers: { cookie: `${SESSION_COOKIE}=admin` } }));
    expect(list.status).toBe(200); expect((await list.json()).users).toHaveLength(2);
    expect((await GET(new NextRequest("https://admin.example/api/users?pageSize=26", { headers: { cookie: `${SESSION_COOKIE}=admin` } }))).status).toBe(400);
  });
  it("creates a host-only secure session and reads persisted user detail", async () => {
    const response = await POST(new Request("https://admin.example/api/session", {
      method: "POST", headers: { origin: "https://admin.example" }, body: JSON.stringify({ token: "id" }),
    }));
    expect(response.status).toBe(200);
    const header = response.headers.get("set-cookie")!;
    expect(header).toContain("HttpOnly"); expect(header).toContain("Secure"); expect(header).toContain("SameSite=lax");
    expect(header).toContain("Path=/"); expect(header).not.toContain("Domain=");
    const cookie = header.split(";")[0];
    const read = await GET(new NextRequest("https://admin.example/api/users?q=user@localhost", { headers: { cookie } }));
    expect(read.status).toBe(200);
    expect(await read.json()).toMatchObject({ user: { uid: "local-user", email: "user@localhost" } });
    const logout = await DELETE(new NextRequest("https://admin.example/api/session", { method: "DELETE", headers: { cookie, origin: "https://admin.example" } }));
    expect(logout.status).toBe(200);
    expect(logout.headers.get("set-cookie")).toContain("Max-Age=0");
  });
  it.each([undefined, "https://attacker.example"])("refuses cross-origin session creation (%s)", async origin => {
    const response = await POST(new Request("https://admin.example/api/session", {
      method: "POST", headers: origin ? { origin } : {}, body: JSON.stringify({ token: "id" }),
    }));
    expect(response.status).toBe(403); expect(backend.sessions.size).toBe(0);
  });
  it("refuses a non-admin ID token", async () => {
    backend.tokens.set("user-id", { uid: "local-user", claims: {}, expiresAt: Date.now() / 1000 + 3600, authenticatedAt: Date.now() / 1000 });
    const response = await POST(new Request("https://admin.example/api/session", {
      method: "POST", headers: { origin: "https://admin.example" }, body: JSON.stringify({ token: "user-id" }),
    }));
    expect(response.status).toBe(403); expect(backend.sessions.size).toBe(0);
  });
});
