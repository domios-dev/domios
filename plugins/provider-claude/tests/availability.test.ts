/**
 * The availability decision as a pure table. This is the cheapest oracle for
 * the whole task: the same two inputs used to collapse into one boolean, so a
 * CLI that could not be reached and a CLI that is genuinely too old were the
 * same answer. They are three outcomes now, and only one of them is a negative.
 *
 * Run: node node_modules/tsx/dist/cli.mjs plugins/provider-claude/tests/availability.test.ts
 *
 * RED BEFORE GREEN: against the collapsed boolean, the "probe could not run"
 * rows below report `unavailable`, which is exactly the shipped bug.
 */

import { decideAvailability, type Availability, type CliProbe } from "../src/models";

let passed = 0;
let failed = 0;

function eq(actual: unknown, expected: unknown, name: string) {
  if (actual === expected) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}: expected ${String(expected)}, got ${String(actual)}`); }
}

const GATE = [2, 1, 258];
const version = (v: number[]): CliProbe => ({ kind: "version", version: v });
const unreachable: CliProbe = { kind: "unreachable" };

console.log("\n--- decideAvailability: three outcomes, not two ---\n");

const table: Array<[string, CliProbe, boolean | undefined, Availability]> = [
  ["a version above the gate is available", version([2, 1, 263]), undefined, "available"],
  ["a version exactly at the gate is available", version([2, 1, 258]), undefined, "available"],
  ["a version below the gate is unavailable", version([2, 1, 257]), undefined, "unavailable"],
  ["an older minor is unavailable", version([2, 0, 999]), undefined, "unavailable"],
  ["a newer major is available", version([3, 0, 0]), undefined, "available"],
  ["a probe that could not run is unknown", unreachable, undefined, "unknown"],
  ["a probe that could not run is never unavailable", unreachable, true, "unknown"],
  ["a definite negative already in hand stays unavailable", unreachable, false, "unavailable"],
  ["a definite negative outranks a satisfied gate", version([2, 1, 263]), false, "unavailable"],
];

for (const [name, probe, declared, expected] of table) {
  eq(decideAvailability(probe, GATE, declared), expected, name);
}

console.log("\n--- the distinction the cache depends on ---\n");

eq(
  decideAvailability(unreachable, GATE) === decideAvailability(version([2, 1, 257]), GATE),
  false,
  "could-not-run and ran-and-answered-no are not the same outcome",
);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
