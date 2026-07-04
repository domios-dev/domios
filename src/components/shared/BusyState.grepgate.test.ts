// Run: npx tsx src/components/shared/BusyState.grepgate.test.ts
//
// Enforces the universal busy grammar: the bespoke-spinner / static-loading
// vocabulary (animate-spin, "Loading...", "Loading…") may exist ONLY inside the
// shared BusyState component. Any other hit is a dead surface that skipped the
// grammar — fix it, don't extend the allowlist.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const srcRoot = join(here, "..", "..");

const ALLOWLIST = new Set([
  "components/shared/BusyState.tsx",
  "components/shared/BusyState.test.tsx",
  "components/shared/BusyState.grepgate.test.ts",
]);

const BANNED = [/animate-spin/, /Loading\.\.\./, /Loading…/];

function walk(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry === "dist") continue;
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) walk(full, out);
    else if (/\.(ts|tsx)$/.test(entry)) out.push(full);
  }
  return out;
}

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

const offenders: string[] = [];
for (const file of walk(srcRoot)) {
  const rel = relative(srcRoot, file).split("\\").join("/");
  if (ALLOWLIST.has(rel)) continue;
  const text = readFileSync(file, "utf8");
  text.split("\n").forEach((line, i) => {
    if (BANNED.some((re) => re.test(line))) {
      offenders.push(`${rel}:${i + 1}: ${line.trim()}`);
    }
  });
}

assert(
  offenders.length === 0,
  `Bespoke spinner / loading text outside shared BusyState:\n${offenders.join("\n")}`,
);

console.log("✓ busy-state grep gate: no bespoke spinners/loading text outside BusyState");
