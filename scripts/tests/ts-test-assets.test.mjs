import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

const hook = new URL("../ts-test-asset-hook.mjs", import.meta.url).href;

for (const extension of ["cjs", "mjs"]) {
  test(`the Node test asset hook stubs CSS imports in ${extension} modules`, (t) => {
    const root = mkdtempSync(path.join(tmpdir(), "ts-test-css-"));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    writeFileSync(path.join(root, "style.css"), ".opaque-fixture { display: none; }\n");
    const entry = path.join(root, `entry.${extension}`);
    const imports = extension === "cjs"
      ? "const assert = require('node:assert/strict'); const style = require('./style.css');"
      : "import assert from 'node:assert/strict'; import style from './style.css';";
    writeFileSync(entry, `${imports}\nassert.deepEqual(style, {}); console.log('css-loaded');\n`);
    const result = spawnSync(process.execPath, ["--import", hook, entry], {
      encoding: "utf8",
      timeout: 10_000,
    });
    assert.equal(result.error, undefined);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /css-loaded/);
  });
}
