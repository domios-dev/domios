/**
 * Host plugin registry tests.
 * Run: npx tsx src/lib/plugins/__tests__/registry.test.ts
 *
 * The React hook `useRegistryVersion` is intentionally not tested here
 * — it requires a React DOM runtime. The pure registry surface
 * (register/unregister/getPlugin/getAllPlugins/detectProvider/
 * isProviderNoise) is fully covered.
 */

import {
  register,
  unregister,
  getPlugin,
  getAllPlugins,
  detectProvider,
  isProviderNoise,
} from "../registry";
import type { ProviderPlugin } from "../types";

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}${detail ? `: ${detail}` : ""}`); }
}

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}, got ${a}`);
}

// --- Fixtures ---

function mkPlugin(id: string, opts: Partial<ProviderPlugin> = {}): ProviderPlugin {
  return {
    meta: { id, displayName: id.toUpperCase() },
    detect: () => false,
    parseLine: () => [],
    statusOf: () => null,
    Renderer: () => null,
    actions: [],
    ...opts,
  };
}

// Each test cleans up so they don't bleed into each other.
function reset() {
  for (const p of getAllPlugins()) unregister(p.meta.id);
}

console.log("\n--- register + getPlugin + getAllPlugins ---\n");

reset();
const foo = mkPlugin("foo");
register(foo);
eq(getPlugin("foo")?.meta.id, "foo", "register + lookup by id");
eq(getPlugin("nonexistent"), undefined, "unknown id → undefined");
eq(getPlugin(null), undefined, "null id → undefined");
eq(getPlugin(undefined), undefined, "undefined id → undefined");

const bar = mkPlugin("bar");
register(bar);
eq(getAllPlugins().length, 2, "getAllPlugins returns both");

// Idempotency: re-register replaces
const fooV2 = mkPlugin("foo", { meta: { id: "foo", displayName: "FOO V2" } });
register(fooV2);
eq(getPlugin("foo")?.meta.displayName, "FOO V2", "re-register overwrites");
eq(getAllPlugins().length, 2, "getAllPlugins count unchanged on re-register");

console.log("\n--- unregister ---\n");

reset();
register(mkPlugin("zap"));
assert(getPlugin("zap") !== undefined, "registered before unregister");
unregister("zap");
eq(getPlugin("zap"), undefined, "unregister removes plugin");
unregister("never_registered"); // no-throw
assert(true, "unregister unknown id does not throw");

console.log("\n--- detectProvider ---\n");

reset();
register(mkPlugin("a", {
  detect: (s) => s.bytes.includes("AAA"),
  titlePattern: /a-title/,
}));
register(mkPlugin("b", {
  detect: (s) => s.bytes.includes("BBB"),
  titlePattern: /b-title/,
}));

eq(detectProvider("hello AAA world"), "a", "detect via bytes — first plugin");
eq(detectProvider("hello BBB world"), "b", "detect via bytes — second plugin");
eq(detectProvider("plain text"), undefined, "no match → undefined");
eq(detectProvider("plain text", "a-title"), "a", "fallback to titlePattern when bytes don't match");
eq(detectProvider("plain text", "b-title"), "b", "titlePattern of second plugin");
eq(detectProvider("plain text", "neither"), undefined, "no title match either → undefined");

// detect() takes precedence over titlePattern when both match
reset();
register(mkPlugin("x", {
  detect: () => true,
  titlePattern: /never/,
}));
eq(detectProvider("anything", "irrelevant"), "x", "detect()=true always wins");

console.log("\n--- isProviderNoise ---\n");

reset();
register(mkPlugin("noisy", {
  noisePatterns: [/^update available/i, /^shift\+tab/i],
}));

assert(isProviderNoise("noisy", "Update available! Run brew upgrade"), "matches noise pattern");
assert(isProviderNoise("noisy", "shift+tab to cycle"), "matches second noise pattern");
assert(!isProviderNoise("noisy", "Real content here"), "non-noise content negative");
assert(isProviderNoise("noisy", ""), "empty string is treated as noise");
assert(isProviderNoise("noisy", "   "), "whitespace-only is treated as noise");

assert(!isProviderNoise("unknown_id", "any text"), "unknown provider → not noise");
assert(!isProviderNoise(null, "any text"), "null id → not noise");

reset();
register(mkPlugin("clean", { noisePatterns: undefined }));
assert(!isProviderNoise("clean", "any text"), "plugin without noisePatterns → not noise");

reset();
console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) (globalThis as { process?: { exit: (n: number) => never } }).process?.exit(1);
