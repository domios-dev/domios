// Caller: build.sh script-guards via tests/catalogue.json node-script-tests-cargo
import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtempSync, mkdirSync, cpSync, rmSync, appendFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = fileURLToPath(new URL("../", import.meta.url));
test("catalog checker rejects stale TypeScript and regenerates it", () => {
  const fixture = mkdtempSync(resolve(tmpdir(), "domios-catalog-"));
  try {
    for (const path of ["prod-docs/ACCOUNT.md", "src-tauri/crates/codeterm-analytics/catalog.schema.json", "src-tauri/crates/codeterm-analytics/src/catalog/generated.rs", "src/lib/productAnalytics.generated.ts"]) {
      mkdirSync(dirname(resolve(fixture, path)), { recursive: true });
      cpSync(resolve(root, path), resolve(fixture, path));
    }
    for (const id of ["claude", "codex", "cursor", "opencode"]) {
      const manifest = `plugins/provider-${id}/plugin.json`;
      mkdirSync(dirname(resolve(fixture, manifest)), { recursive: true });
      cpSync(resolve(root, manifest), resolve(fixture, manifest));
    }
    const run = (...args) => spawnSync(process.execPath, [resolve(root, "scripts/check-analytics-catalog.mjs"), "--root", fixture, ...args], { encoding: "utf8" });
    assert.equal(run().status, 0);
    appendFileSync(resolve(fixture, "src/lib/productAnalytics.generated.ts"), "// stale\n");
    const stale = run();
    assert.equal(stale.status, 1);
    assert.match(stale.stderr, /Stale analytics catalog: src\/lib\/productAnalytics.generated.ts/);
    assert.equal(run("--write").status, 0);
    assert.equal(run().status, 0);
  } finally { rmSync(fixture, { recursive: true, force: true }); }
});
