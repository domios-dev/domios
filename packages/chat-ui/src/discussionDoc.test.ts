// Run: npx tsx packages/chat-ui/src/discussionDoc.test.ts
import { parseAttachedDiscussionDoc } from "./discussionDoc";

let passed = 0;
let failed = 0;

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

eq(
  parseAttachedDiscussionDoc("doc: notes/decision.md\n\n# Title\n\nBody"),
  { summary: "", path: "notes/decision.md", body: "# Title\n\nBody" },
  "parses doc-only payload",
);

eq(
  parseAttachedDiscussionDoc("STATUS: done\nCHANGES: x\n\ndoc: a.md\n\n# Discussion"),
  { summary: "STATUS: done\nCHANGES: x", path: "a.md", body: "# Discussion" },
  "parses summary + doc tail",
);

eq(parseAttachedDiscussionDoc("plain milestone text"), null, "returns null without doc marker");
eq(parseAttachedDiscussionDoc("doc: empty.md\n\n   "), null, "returns null on empty body");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
