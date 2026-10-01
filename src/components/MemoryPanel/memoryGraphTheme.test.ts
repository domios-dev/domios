import { deriveMemoryGraphPalette } from "./memoryGraphTheme";

let passed = 0;
let failed = 0;

function assert(label: string, ok: boolean, detail?: string) {
  if (ok) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}${detail ? ` — ${detail}` : ""}`);
    failed++;
  }
}

console.log("memoryGraphTheme tests");

{
  const palette = deriveMemoryGraphPalette("#f6f8fa", "#1f2328");
  assert("light theme uses foreground RGB for edges", palette.edgeRgb === "31,35,40");
  assert("light theme detail edges are visible", palette.realEdgeAlpha(0.5) >= 0.14);
  assert("light theme labels use foreground", palette.labelColor === "#1f2328");
}

{
  const palette = deriveMemoryGraphPalette("#0d1117", "#c9d1d9");
  assert("dark theme uses foreground RGB for edges", palette.edgeRgb === "201,209,217");
  assert("dark theme detail edges stay subtle", palette.realEdgeAlpha(0.5) < 0.1);
  assert("dark theme labels use foreground", palette.labelColor === "#c9d1d9");
}

{
  const palette = deriveMemoryGraphPalette("#0d1117", "#c9d1d9");
  assert(
    "edge colors are premultiplied for sigma's blending",
    palette.edgeColor(0.5) === "rgba(101,105,109,0.5)",
    palette.edgeColor(0.5),
  );
  assert("an opaque edge keeps the foreground", palette.edgeColor(1) === "rgba(201,209,217,1)");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
