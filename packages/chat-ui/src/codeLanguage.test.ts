// Run: npx tsx packages/chat-ui/src/codeLanguage.test.ts

import { inferCodeLanguage, languageForEdits, languageFromPath, knownLanguageFromPath, languageFromInfoString } from "./codeLanguage";

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

for (const path of ["oracle.mjs", "C:\\work\\ORACLE.CJS", "src/module.js"]) {
  assert(`JavaScript module ${path}`, knownLanguageFromPath(path) === "javascript");
  assert(`inferred JavaScript module ${path}`, inferCodeLanguage(path, "") === "javascript");
}
for (const path of ["module.mts", "module.cts"]) assert(`TypeScript module ${path}`, knownLanguageFromPath(path) === "typescript");
assert("unknown path stays plain", knownLanguageFromPath("notes.unknown") === "text");
assert("ts fence metadata", languageFromInfoString(" TS title=example ") === "typescript");
assert("js fence alias", languageFromInfoString("js") === "javascript");
assert("empty fence stays plain", languageFromInfoString("") === "text");
assert("lazy grammar preserved", languageFromInfoString("HASKELL") === "haskell");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
