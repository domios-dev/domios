// Run: npx tsx src/components/Canvas/worlds.test.ts

import { CRISP_MIN_SCALE, crispRasterScale } from "./worlds";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

// §34: only readable bands (W2/W3) re-rasterize; mass/structure bands stay 1×.
test("crispRasterScale: birds-eye bands never re-rasterize", () => {
  assert(crispRasterScale("W0", 4) === 1, "W0 stays 1×");
  assert(crispRasterScale("W1", 4) === 1, "W1 stays 1×");
});

test("crispRasterScale: readable bands quantize to 0.25 steps above the min scale", () => {
  assert(crispRasterScale("W2", 1) === 1, "no crisp step at 1× (GPU upscale imperceptible)");
  assert(crispRasterScale("W2", CRISP_MIN_SCALE) === 1, "no crisp step at the threshold itself");
  assert(crispRasterScale("W2", 2) === 2, "W2 quantizes to 2× at 2×");
  assert(crispRasterScale("W2", 3.5) === 3.5, "W2 tracks the dive, quantized to 0.25 steps");
  assert(crispRasterScale("W2", 3.6) === 3.5, "W2 rounds to the nearest 0.25 step");
  assert(crispRasterScale("W3", 4) === 4, "W3 quantizes to 4× at 4×");
});

// Quantization bounds the reflow count across a zoom sweep; geometry still cancels
// (zoom × 1/zoom = 1) because the SAME quantized value drives both sides.
test("crispRasterScale: quantizes to 0.25 steps", () => {
  assert(crispRasterScale("W3", 2.7) === 2.75, "2.7 → 2.75");
  assert(crispRasterScale("W3", 3.12) === 3.0, "3.12 → 3.0");
  assert(crispRasterScale("W3", 3.13) === 3.25, "3.13 → 3.25");
  // Never below 1× even if a rounding edge dips under it.
  assert(crispRasterScale("W2", 1.1) >= 1, "stays ≥ 1×");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failed++;
    console.error(`  ✗ ${name}`);
    console.error(`    ${e instanceof Error ? e.message : e}`);
  }
}
console.log(`\n${tests.length - failed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
