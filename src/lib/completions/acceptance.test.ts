import { getCompletionInsert, isSafeCompletionText } from "./acceptance";
import type { CompletionItem, ParsedInput } from "./types";

let passed = 0;
let failed = 0;

function assertEq<T>(label: string, actual: T, expected: T) {
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

function parsed(currentArg: string): ParsedInput {
  return {
    command: "codeterm",
    args: [],
    currentArg,
    cursor: currentArg.length,
    currentArgStart: 0,
    currentArgEnd: currentArg.length,
  };
}

function item(text: string): CompletionItem {
  return {
    text,
    display: text,
    description: "",
    kind: "subcommand",
  };
}

console.log("completion acceptance:");

assertEq("normal prefix inserts suffix only", getCompletionInsert(parsed("wor"), item("workspace")), "kspace ");
assertEq("case-insensitive prefix inserts original suffix", getCompletionInsert(parsed("Work"), item("workspace")), "space ");
assertEq("path prefix after deletion inserts suffix only", getCompletionInsert(parsed("./My"), item("./My Folder")), " Folder ");
assertEq("special chars stay literal when safe", getCompletionInsert(parsed("'./My Dir/a&"), item("'./My Dir/a&b'")), "b' ");
assertEq("non-prefix candidate is rejected", getCompletionInsert(parsed("abc"), item("workspace")), null);
assertEq("escape bytes are unsafe", isSafeCompletionText("work\x1b[31mspace"), false);
assertEq("control bytes are unsafe", isSafeCompletionText("work\x7fspace"), false);
assertEq("plain text is safe", isSafeCompletionText("workspace"), true);

if (failed > 0) {
  throw new Error(`${failed} completion acceptance test(s) failed`);
}

console.log(`acceptance tests: ${passed}/${passed + failed} passed`);
