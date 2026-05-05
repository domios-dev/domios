import { quotePathForPaste } from "./clipboardPaste";

let passed = 0;
let failed = 0;

function assert(label: string, actual: string, expected: string) {
  if (actual === expected) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}: expected "${expected}", got "${actual}"`);
    failed++;
  }
}

console.log("quotePathForPaste:");

assert("plain ascii unchanged",
  quotePathForPaste("/tmp/foo.png"),
  "/tmp/foo.png");

assert("space → single-quote wrap",
  quotePathForPaste("/Users/me/Hello World.png"),
  "'/Users/me/Hello World.png'");

assert("cyrillic + space → single-quote wrap (utf-8 preserved)",
  quotePathForPaste("/Users/rollacode/Desktop/Снимок экрана 2026-05-05 в 10.49.45.png"),
  "'/Users/rollacode/Desktop/Снимок экрана 2026-05-05 в 10.49.45.png'");

assert("windows backslash without space → unchanged (cmd.exe compatibility)",
  quotePathForPaste("C:\\Users\\me\\file.png"),
  "C:\\Users\\me\\file.png");

assert("windows backslash + space → single-quote wrap",
  quotePathForPaste("C:\\Users\\me\\file with space.png"),
  "'C:\\Users\\me\\file with space.png'");

assert("windows path with $ → quoted",
  quotePathForPaste("C:\\$Recycle.Bin\\file.png"),
  "'C:\\$Recycle.Bin\\file.png'");

assert("internal single quote → double-quote wrap with escapes",
  quotePathForPaste("/tmp/it's broken.png"),
  '"/tmp/it\'s broken.png"');

assert("dollar sign in path → quoted",
  quotePathForPaste("/tmp/$foo.png"),
  "'/tmp/$foo.png'");

assert("ampersand → quoted",
  quotePathForPaste("/tmp/a&b.png"),
  "'/tmp/a&b.png'");

assert("parens → quoted",
  quotePathForPaste("/tmp/v(1).png"),
  "'/tmp/v(1).png'");

assert("hash → quoted",
  quotePathForPaste("/tmp/#draft.png"),
  "'/tmp/#draft.png'");

assert("tab unchanged when only safe chars",
  quotePathForPaste("/tmp/safe-name_123.png"),
  "/tmp/safe-name_123.png");

assert("dot leading still safe",
  quotePathForPaste("/tmp/.hidden.png"),
  "/tmp/.hidden.png");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
