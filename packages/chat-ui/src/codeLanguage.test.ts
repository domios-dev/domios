// Run: npx tsx packages/chat-ui/src/codeLanguage.test.ts

import { inferCodeLanguage, languageForEdits, languageFromPath } from "./codeLanguage";

let passed = 0;
let failed = 0;

function assert(label: string, ok: boolean) {
  if (ok) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

console.log("codeLanguage tests");

assert("path tsx", languageFromPath("src/Foo.tsx") === "tsx");
assert(
  "edit without path infers tsx",
  inferCodeLanguage(null, 'import React from "react";\nconst x = <button className="a" />;') === "tsx",
);
assert(
  "languageForEdits uses path first",
  languageForEdits("lib/main.rs", [{ old: "fn a()", new: "fn b()" }]) === "rust",
);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
