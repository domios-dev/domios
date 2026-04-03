import { parseHex, blendColor, deriveColor, adjustBrightness, rgbToHsl, hslToRgb } from "./colors";

let passed = 0;
let failed = 0;

function assert(label: string, actual: unknown, expected: unknown) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}: expected ${e}, got ${a}`);
    failed++;
  }
}

function assertHex(label: string, actual: string, expected: string) {
  if (actual.toLowerCase() === expected.toLowerCase()) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}: expected ${expected}, got ${actual}`);
    failed++;
  }
}

// ── parseHex ──────────────────────────────────────────────────────

console.log("parseHex:");
assert("black", parseHex("#000000"), [0, 0, 0]);
assert("white", parseHex("#ffffff"), [255, 255, 255]);
assert("red", parseHex("#ff0000"), [255, 0, 0]);
assert("no hash", parseHex("00ff00"), [0, 255, 0]);

// ── blendColor ────────────────────────────────────────────────────

console.log("\nblendColor:");
assertHex("0 ratio = colorA", blendColor("#000000", "#ffffff", 0), "#000000");
assertHex("1 ratio = colorB", blendColor("#000000", "#ffffff", 1), "#ffffff");
assertHex("0.5 = midpoint", blendColor("#000000", "#ffffff", 0.5), "#808080");

// ── deriveColor ───────────────────────────────────────────────────

console.log("\nderiveColor:");
const darkDerived = deriveColor("#0d1117", 10);
const lightDerived = deriveColor("#f0f0f0", 10);
// Dark color should get lighter
assert("dark gets lighter", parseHex(darkDerived)[0] > parseHex("#0d1117")[0], true);
// Light color should get darker
assert("light gets darker", parseHex(lightDerived)[0] < parseHex("#f0f0f0")[0], true);

// ── adjustBrightness ─────────────────────────────────────────────

console.log("\nadjustBrightness:");
const darkAdj = adjustBrightness("#0d1117", 20);
const lightAdj = adjustBrightness("#f0f0f0", 20);
assert("dark adj gets lighter", parseHex(darkAdj)[0] > parseHex("#0d1117")[0], true);
assert("light adj gets darker", parseHex(lightAdj)[0] < parseHex("#f0f0f0")[0], true);

// ── rgbToHsl / hslToRgb roundtrip ────────────────────────────────

console.log("\nrgbToHsl/hslToRgb roundtrip:");
const testColors: [number, number, number][] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [128, 128, 128], [0, 0, 0], [255, 255, 255]];
for (const [r, g, b] of testColors) {
  const [h, s, l] = rgbToHsl(r, g, b);
  const [rr, gg, bb] = hslToRgb(h, s, l);
  const ok = Math.abs(r - rr) <= 1 && Math.abs(g - gg) <= 1 && Math.abs(b - bb) <= 1;
  if (ok) {
    console.log(`  ✓ roundtrip (${r},${g},${b})`);
    passed++;
  } else {
    console.error(`  ✗ roundtrip (${r},${g},${b}): got (${rr},${gg},${bb})`);
    failed++;
  }
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
