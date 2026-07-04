
// Grep-gate: bespoke button/chip/card primitives must not accumulate outside shared/.
// Baseline counts are locked; the allowlist only shrinks. A planted fixture in the test
// itself exercises the red path so the gate can't be silently broken by deleting patterns.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const srcRoot = join(here, "..", "..");

// Files that own or test the canonical primitives — exempt from all gates.
const ALLOWLIST = new Set([
  "components/shared/Button.tsx",
  "components/shared/Button.test.tsx",
  "components/shared/Chip.tsx",
  "components/shared/Chip.test.tsx",
  "components/shared/Card.tsx",
  "components/shared/Card.test.tsx",
  "components/shared/FormRow.tsx",
  "components/shared/FormRow.test.tsx",
  "components/shared/shared.grepgate.test.ts",
  "components/shared/design.ts",
  // BusyState primitives
  "components/shared/BusyState.tsx",
  "components/shared/BusyState.test.tsx",
  "components/shared/BusyState.grepgate.test.ts",
  // Gallery renders all variants for QA
  "components/Gallery/ComponentGallery.tsx",
]);

// Patterns that indicate a bespoke button/chip/card built outside the shared layer.
// Each pattern is paired with a human-readable name for the error message.
const BANNED: Array<[RegExp, string]> = [
  // bespoke rounded-full pill with explicit px + py — the canonical chip shape
  [/rounded-full.*px-\d+.*py-\d+|px-\d+.*py-\d+.*rounded-full/, "bespoke rounded-full chip"],
  // bespoke inline-flex + rounded + border that looks like a button primitive
  [/inline-flex.*rounded.*border[^-]|rounded.*border[^-].*inline-flex/, "bespoke inline button"],
];

// Baseline counts (locked at design-audit snapshot); gate fails if exceeded.
// Run `npx tsx shared.grepgate.test.ts --update-baseline` to unlock an intentional bump.
const BASELINE: Record<string, number> = {
  "bespoke rounded-full chip": 38,
  "bespoke inline button": 26,
};

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

// ── Planted fixture: verify the gate detects bespoke patterns ────────────────
// This string is intentionally never inside a real component so the grep can't
// false-negative. The outer test validates the scan logic before trusting the
// allowlist path.
const PLANTED_CHIP = `<span className="inline-flex items-center px-2 py-0.5 rounded-full border">tag</span>`;
const PLANTED_BUTTON = `<button className="inline-flex rounded border px-3">click</button>`;

function countMatches(text: string, re: RegExp): number {
  return (text.match(re) ? 1 : 0);
}

// Planted fixture must fire.
assert(
  countMatches(PLANTED_CHIP, BANNED[0][0]) > 0,
  "planted chip fixture must match the chip ban pattern",
);
assert(
  countMatches(PLANTED_BUTTON, BANNED[1][0]) > 0,
  "planted button fixture must match the button ban pattern",
);

// ── Main scan ────────────────────────────────────────────────────────────────
const counts: Record<string, number> = {};
for (const [, name] of BANNED) counts[name] = 0;

for (const file of walk(srcRoot)) {
  const rel = relative(srcRoot, file).split("\\").join("/");
  if (ALLOWLIST.has(rel)) continue;
  const text = readFileSync(file, "utf8");
  for (const [re, name] of BANNED) {
    text.split("\n").forEach((line) => {
      if (re.test(line)) counts[name]++;
    });
  }
}

// ── Baseline enforcement ─────────────────────────────────────────────────────
const failures: string[] = [];
for (const [, name] of BANNED) {
  const baseline = BASELINE[name] ?? 0;
  const actual = counts[name];
  if (actual > baseline) {
    failures.push(`  "${name}": ${actual} occurrences (baseline ${baseline}) — migrate new ones to shared/Chip or shared/Button`);
  }
}

assert(
  failures.length === 0,
  `Bespoke primitive count exceeded baseline:\n${failures.join("\n")}\n\nMigrate new bespoke patterns to shared/ or update BASELINE if intentional.`,
);

console.log(
  `✓ shared-primitive grep gate: counts within baseline`,
  Object.entries(counts).map(([k, v]) => `${k}=${v}/${BASELINE[k]}`).join(", "),
);
