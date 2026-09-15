/**
 * Codex directory-trust prompt response over wrapped trust frames.
 * Run: npx tsx plugins/provider-codex/tests/directory-trust-frames.test.ts
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import plugin from "../src/plugin";

let passed = 0;
let failed = 0;

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}: expected ${e}, got ${a}`); }
}

console.log("\n--- directoryTrustPromptResponse: wrapped Codex trust frames ---\n");

{
  const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), "fixtures", "directory-trust");
  const cases = JSON.parse(fs.readFileSync(path.join(dir, "cases.json"), "utf8")) as Array<{ file: string; expect: string }>;
  const keys: Record<string, string[] | null> = {
    enter: ["DQ=="],
    "move-up": ["G1tB"],
    "move-down": ["G1tC"],
    hold: [],
    none: null,
  };
  eq(cases.length >= 5, true, "every declared Codex directory-trust fixture is exercised");
  for (const c of cases) {
    const screen = fs.readFileSync(path.join(dir, c.file), "utf8");
    eq(plugin.directoryTrustPromptResponse!(screen), keys[c.expect], `codex directory trust ${c.file} -> ${c.expect}`);
  }
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
