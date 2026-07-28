import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const fixture = mkdtempSync(path.join(tmpdir(), "codeterm-knip-"));
const src = path.join(fixture, "src");
mkdirSync(src);

try {
  writeFileSync(
    path.join(fixture, "package.json"),
    JSON.stringify({ name: "unused-export-fixture", private: true, type: "module" }),
  );
  writeFileSync(
    path.join(fixture, "tsconfig.json"),
    JSON.stringify({ compilerOptions: { module: "ESNext", moduleResolution: "bundler" } }),
  );
  writeFileSync(
    path.join(fixture, "knip.json"),
    JSON.stringify({
      entry: ["src/main.ts", "src/tauriBindings.ts", "src/**/*.test.ts"],
      project: ["src/**/*.ts"],
    }),
  );
  writeFileSync(
    path.join(src, "main.ts"),
    'import { liveExport } from "./module";\nexport function frameworkEntry() { return liveExport(); }\n',
  );
  writeFileSync(
    path.join(src, "module.ts"),
    "export const liveExport = () => 1;\nexport const seededDeadExport = () => 2;\n",
  );
  writeFileSync(
    path.join(src, "tauriBindings.ts"),
    "export const invokeTauriCommand = () => 'invoke';\n",
  );
  writeFileSync(
    path.join(src, "feature.test.ts"),
    "export const testFixtureHelper = () => 'fixture';\n",
  );

  const result = spawnSync(
    process.execPath,
    [path.join(root, "node_modules/knip/bin/knip.js"), "--exports", "--reporter", "compact"],
    { cwd: fixture, encoding: "utf8" },
  );
  const output = `${result.stdout}\n${result.stderr}`;
  assert.notEqual(result.status, 0, "seeded dead export must fail the gate");
  assert.match(output, /seededDeadExport/);
  assert.doesNotMatch(output, /frameworkEntry/);
  assert.doesNotMatch(output, /invokeTauriCommand/);
  assert.doesNotMatch(output, /testFixtureHelper/);
  console.log("unused-export gate regression checks passed");
} finally {
  rmSync(fixture, { recursive: true, force: true });
}
