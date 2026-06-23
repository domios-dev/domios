import { InputTracker } from "./inputTracker";

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

function feed(data: string): InputTracker {
  const tracker = new InputTracker();
  tracker.feed(data);
  return tracker;
}

console.log("input tracker:");

{
  const tracker = feed("codeterm wor");
  assertEq("tracks normal prefix line", tracker.getLine(), "codeterm wor");
  assertEq("tracks normal prefix cursor", tracker.getCursor(), 12);
  assertEq("parses normal prefix", tracker.getCursorWord()?.currentArg, "wor");
}

{
  const tracker = feed("codeterm worxk\x1b[D\x7f");
  assertEq("backspace edits before cursor", tracker.getLine(), "codeterm work");
  assertEq("backspace keeps cursor at edit point", tracker.getCursor(), 12);
  assertEq("parses prefix after backspace edit", tracker.getCursorWord()?.currentArg, "wor");
}

{
  const tracker = feed("codeterm worrk\x1b[D\x1b[D\x1b[3~");
  assertEq("delete removes character at cursor", tracker.getLine(), "codeterm work");
  assertEq("parses prefix after delete edit", tracker.getCursorWord()?.currentArg, "wor");
}

{
  const tracker = feed("codeterm work\x01echo \x05space");
  assertEq("ctrl-a/e edits line and cursor", tracker.getLine(), "echo codeterm workspace");
  assertEq("ctrl-a/e leaves cursor at end after final insert", tracker.getCursor(), 23);
}

{
  const tracker = feed("codeterm workspace --json\x17--help");
  assertEq("ctrl-w deletes word before cursor", tracker.getLine(), "codeterm workspace --help");
}

{
  const tracker = feed("codeterm workspace list\x01\x0bidentify");
  assertEq("ctrl-k deletes to end", tracker.getLine(), "identify");
}

{
  const tracker = feed("codeterm workspace\x15identify");
  assertEq("ctrl-u deletes before cursor", tracker.getLine(), "identify");
}

{
  const tracker = feed("codeterm worksp\x7f");
  assertEq("path-like prefix after deletion remains current visible token", tracker.getCursorWord()?.currentArg, "works");
}

{
  const tracker = feed("codeterm open './My Dir/a&");
  assertEq("quoted token with spaces remains one current arg", tracker.getCursorWord()?.currentArg, "'./My Dir/a&");
}

{
  const tracker = feed("codeterm wo\x1b[200~junk");
  assertEq("unsupported bracketed paste sequence loses confidence", tracker.isConfident(), false);
  assertEq("unconfident tracker does not parse completion text", tracker.getCursorWord(), null);
}

if (failed > 0) {
  throw new Error(`${failed} input tracker test(s) failed`);
}

console.log(`inputTracker tests: ${passed}/${passed + failed} passed`);
