import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { HostAnalytics } from "@codeterm/plugin-sdk";
import plugin, { runQuery } from "../src/plugin";
import { snapshotFor } from "../ui/src/model/fixtures.testdata";

const QUERY = { range: { from: "2026-09-09", to: "2026-09-15", tz: "UTC" }, bucket: "day", group_by: "workspace" };

function analytics(answer: unknown): HostAnalytics {
  return { query: vi.fn(() => answer) } as unknown as HostAnalytics;
}

afterEach(() => {
  delete (globalThis as { host?: unknown }).host;
});

describe("analytics plugin viewCall", () => {
  it("passes the host snapshot through unchanged", () => {
    const snapshot = snapshotFor("workspace");
    const host = analytics(snapshot);
    expect(runQuery(QUERY, host)).toEqual({ kind: "snapshot", snapshot });
    expect(host.query).toHaveBeenCalledWith(QUERY);
  });

  it("reports no answer when the host is absent or answers null, never an empty snapshot", () => {
    expect(runQuery(QUERY, undefined)).toEqual({ kind: "unavailable" });
    expect(runQuery(QUERY, analytics(null))).toEqual({ kind: "unavailable" });
    expect(runQuery(QUERY, analytics({ unexpected: true }))).toEqual({ kind: "unavailable" });
  });

  it("carries a refusal's message, including a missing permission", () => {
    expect(runQuery(QUERY, analytics({ error: "foreign_scope", message: "plan pl_x is not held by this machine" }))).toEqual({
      kind: "refused",
      message: "plan pl_x is not held by this machine",
    });
    expect(runQuery(QUERY, analytics({ error: "host.analytics: analytics permission required" }))).toEqual({
      kind: "refused",
      message: "host.analytics: analytics permission required",
    });
  });

  it("refuses a malformed query without asking the host", () => {
    const host = analytics(null);
    expect(runQuery("range=7d", host)).toEqual({ kind: "refused", message: "query must be an object" });
    expect(host.query).not.toHaveBeenCalled();
  });

  it("routes the bridge's query method to the injected host and refuses other methods", () => {
    const snapshot = snapshotFor("provider_model");
    (globalThis as { host?: unknown }).host = { analytics: analytics(snapshot) };
    const viewCall = plugin.viewCall!;
    expect(viewCall("query", { query: QUERY })).toEqual({ kind: "snapshot", snapshot });
    expect(viewCall("readLedger", {})).toEqual({ kind: "refused", message: "unknown method: readLedger" });
  });

  it("keeps plugin and view code free of provider ids, raw storage and ledger paths", () => {
    const root = join(dirname(fileURLToPath(import.meta.url)), "..");
    const files: string[] = [];
    const walk = (dir: string) => {
      for (const entry of readdirSync(dir)) {
        const full = join(dir, entry);
        if (statSync(full).isDirectory()) walk(full);
        else if (/\.(ts|tsx)$/.test(entry) && !/\.test\.|\.testdata\./.test(entry)) files.push(full);
      }
    };
    walk(join(root, "src"));
    walk(join(root, "ui", "src"));
    expect(files.length).toBeGreaterThan(5);
    for (const file of files) {
      const text = readFileSync(file, "utf8");
      expect(text, file).not.toMatch(/["'`](claude|codex|opencode|cursor)["'`]/i);
      expect(text, file).not.toMatch(/sqlite|kpi_ledger|usage\.db|readFile|host\.fs/i);
    }
  });
});
