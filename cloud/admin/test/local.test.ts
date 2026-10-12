import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { LocalBackend } from "../src/adapters/local";
import { providerName } from "../src/adapters";
import { createAdminSession, requireAdmin } from "../src/domain/auth";
import { AdminService } from "../src/domain/service";

describe("local adapter", () => {
  it("signs in and persists grants and revocations across adapter restarts", async () => {
    const directory = mkdtempSync(join(tmpdir(), "domios-admin-"));
    try {
      const path = join(directory, "state.json");
      const first = new LocalBackend(path, "test-secret", "password");
      const token = await first.login("admin@localhost", "password");
      const cookie = await createAdminSession(first.auth, token);
      const service = new AdminService(first, ["pro"]);
      const granted = await service.change(cookie, "local-user", "grant", "pro", "Local grant");
      const second = new LocalBackend(path, "test-secret", "password");
      expect((await second.detail("local-user"))?.entitlement).toEqual(granted.after);
      const revoked = await new AdminService(second, ["pro"]).change(cookie, "local-user", "revoke", "", "Local revoke");
      const third = new LocalBackend(path, "test-secret", "password");
      expect((await third.detail("local-user"))?.audit).toEqual([revoked, granted]);
      expect((await third.detail("local-user"))?.entitlement.status).toBe("revoked");
      await expect(requireAdmin(third.auth, cookie + "tampered")).rejects.toMatchObject({ status: 403 });
      await expect(requireAdmin(third.auth, token)).rejects.toMatchObject({ status: 403 });
      await expect(first.login("admin@localhost", "wrong")).rejects.toMatchObject({ status: 403 });
    } finally { rmSync(directory, { recursive: true, force: true }); }
  });
  it("prevents development providers on deployed or production processes", () => {
    expect(() => providerName({ NODE_ENV: "production", ADMIN_PROVIDER: "local" })).toThrow();
    expect(() => providerName({ VERCEL: "1", ADMIN_PROVIDER: "fake" })).toThrow();
    expect(providerName({ NODE_ENV: "production" })).toBe("firebase");
    expect(() => providerName({ ADMIN_PROVIDER: "unknown" })).toThrow();
  });
});
