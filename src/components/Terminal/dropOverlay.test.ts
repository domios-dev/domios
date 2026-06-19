import { shouldShowDropOverlay } from "./dropOverlay";

let passed = 0;
let failed = 0;

function assertBool(label: string, actual: boolean, expected: boolean) {
  if (actual === expected) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}: expected ${expected}, got ${actual}`);
    failed++;
  }
}

console.log("terminal drop overlay:");

assertBool(
  "shows for file drag when terminal is ready",
  shouldShowDropOverlay({ isFileDragOver: true, isTextDragOver: false, termReady: true }),
  true,
);

assertBool(
  "shows for text drag when terminal is ready",
  shouldShowDropOverlay({ isFileDragOver: false, isTextDragOver: true, termReady: true }),
  true,
);

assertBool(
  "does not require terminal uiMode",
  shouldShowDropOverlay({ isFileDragOver: true, isTextDragOver: false, termReady: true }),
  true,
);

assertBool(
  "stays hidden until terminal is ready",
  shouldShowDropOverlay({ isFileDragOver: true, isTextDragOver: true, termReady: false }),
  false,
);

if (failed > 0) {
  throw new Error(`${failed} drop overlay test(s) failed`);
}

console.log(`dropOverlay tests: ${passed}/${passed + failed} passed`);
