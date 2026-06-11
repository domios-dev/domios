// Run: npx tsx packages/chat-core/src/markers.test.ts
import { MARKER_OPEN, parseMarker } from "./markers";

let passed = 0,
  failed = 0;
function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}: expected ${e}, got ${a}`);
  }
}

const leadingSpaceMarker =
  ` ${MARKER_OPEN}pane label=main pane=bda0e267 model=Opus-=-Payload with space`;
const parsed = parseMarker(leadingSpaceMarker);

eq(parsed?.kind, "pane", "parseMarker strips leading whitespace");
eq(parsed?.attrs, { label: "main", pane: "bda0e267", model: "Opus" }, "leading-space marker attrs");
eq(parsed?.payload, "Payload with space", "leading-space marker payload");

const normal = parseMarker(`${MARKER_OPEN}pane label=clean-=-ok`);
eq(normal?.kind, "pane", "parseMarker keeps no-leading-space case");
eq(normal?.payload, "ok", "parseMarker keeps payload unchanged");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
