
import { cleanMarkdown } from "./drawCard";

let passed = 0;
let failed = 0;
function ok(label: string, cond: unknown) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

ok("strips inline code backticks", cleanMarkdown("run `codeterm plan`").includes("run codeterm plan"));
ok("strips bold markers", cleanMarkdown("this is **important**") === "this is important");
ok("strips strikethrough markers", cleanMarkdown("~~old~~ new") === "old new");
ok("strips heading marks", cleanMarkdown("## Overview") === "Overview");
ok("links collapse to their text", cleanMarkdown("see [the docs](https://x.io/y)") === "see the docs");
ok("images collapse to alt text", cleanMarkdown("![diagram](a.png)") === "diagram");
ok("blockquote marks removed", cleanMarkdown("> quoted line") === "quoted line");
ok("bullets normalized to •", cleanMarkdown("- first\n* second\n+ third") === "• first\n• second\n• third");
ok("underscores in identifiers preserved", cleanMarkdown("task_body and section_ids") === "task_body and section_ids");
ok("plain prose untouched", cleanMarkdown("just a sentence.") === "just a sentence.");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
