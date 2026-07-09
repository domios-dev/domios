import { readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const srcRoot = join(here, "..", "..");

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
  "components/shared/BusyState.tsx",
  "components/shared/BusyState.test.tsx",
  "components/shared/BusyState.grepgate.test.ts",
  "components/Gallery/ComponentGallery.tsx",
]);

const BANNED: Array<[RegExp, string]> = [
  [/rounded-full.*px-\d+.*py-\d+|px-\d+.*py-\d+.*rounded-full/, "bespoke rounded-full chip"],
  [/inline-flex.*rounded.*border[^-]|rounded.*border[^-].*inline-flex/, "bespoke inline button"],
];

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

const PLANTED_CHIP = `<span className="inline-flex items-center px-2 py-0.5 rounded-full border">tag</span>`;
const PLANTED_BUTTON = `<button className="inline-flex rounded border px-3">click</button>`;

function countMatches(text: string, re: RegExp): number {
  return (text.match(re) ? 1 : 0);
}

assert(
  countMatches(PLANTED_CHIP, BANNED[0][0]) > 0,
  "planted chip fixture must match the chip ban pattern",
);
assert(
  countMatches(PLANTED_BUTTON, BANNED[1][0]) > 0,
  "planted button fixture must match the button ban pattern",
);

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
