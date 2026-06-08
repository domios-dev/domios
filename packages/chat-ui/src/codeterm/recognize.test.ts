// Run: npx tsx packages/chat-ui/src/codeterm/recognize.test.ts
import { recognize } from "./recognize";

let passed = 0,
  failed = 0;
function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual),
    e = JSON.stringify(expected);
  if (a === e) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}: expected ${e}, got ${a}`);
  }
}
function ok(c: boolean, name: string) {
  eq(c, true, name);
}

console.log("\n--- recognize ---\n");

const inv = recognize('codeterm send-text --pane bda0e267 "Понял, буду"');
eq(inv?.path, ["send-text"], "send-text path");
eq(inv?.flags.pane, "bda0e267", "send-text --pane flag");
eq(inv?.positionals, ["Понял, буду"], "send-text positional payload");

eq(recognize('codeterm agent spawn claude --task "go"')?.path, ["agent", "spawn"], "agent spawn path");
eq(recognize("CODETERM_API_PORT=7686 codeterm pane list")?.path, ["pane", "list"], "env-prefix stripped");
ok(recognize("~/.codeterm/bin/codeterm mem search foo") !== null, "abs path binary recognized");
ok(recognize("ls -la") === null, "non-codeterm → null");
ok(recognize('echo hi | codeterm send-text "x"') === null, "buried after pipe → null");
ok(recognize("codeterm-dev pane list") !== null, "codeterm-dev recognized");
eq(recognize("codeterm cron list --json")?.path, ["cron", "list"], "known depth-2 subcommand");
eq(recognize("codeterm agent spawn claude")?.positionals, ["claude"], "trailing word stays positional");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
