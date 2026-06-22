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

const inv = recognize('codeterm send --pane bda0e267 "Понял, буду"');
eq(inv?.path, ["send"], "send path");
eq(inv?.flags.pane, "bda0e267", "send --pane flag");
eq(inv?.positionals, ["Понял, буду"], "send positional payload");

eq(recognize('codeterm agent spawn claude --task "go"')?.path, ["agent", "spawn"], "agent spawn path");
eq(recognize("CODETERM_API_PORT=7686 codeterm pane list")?.path, ["pane", "list"], "env-prefix stripped");
ok(recognize("~/.codeterm/bin/codeterm mem search foo") !== null, "abs path binary recognized");
ok(recognize("ls -la") === null, "non-codeterm → null");
ok(recognize('echo hi | codeterm send "x"') === null, "buried after pipe → null");
ok(recognize("codeterm-dev pane list") !== null, "codeterm-dev recognized");
eq(recognize("codeterm cron list --json")?.path, ["cron", "list"], "known depth-2 subcommand");
eq(recognize("codeterm agent spawn claude")?.positionals, ["claude"], "trailing word stays positional");

console.log("\n--- templates ---\n");
import { renderTemplate } from "./templates";

const sent = recognize('codeterm send --pane bda0e267 "hi there"')!;
eq(renderTemplate(sent).title, "Sent to pane bda0e267", "send title");
eq(renderTemplate(sent, "OK (sent)").status, { ok: true, label: "delivered" }, "send status from result");

const sendAlias = recognize('codeterm send --pane keats "Привет от send"')!;
eq(renderTemplate(sendAlias).title, "Sent to pane keats", "send title from renamed command");
eq(renderTemplate(sendAlias, "OK").status, { ok: true, label: "delivered" }, "send status from result");

const resolveName = (ref: string) => (ref.slice(0, 8) === "45509574" ? "qa-lat" : undefined);
const sentResolvable = recognize('codeterm send --pane 45509574 "yo"')!;
eq(renderTemplate(sentResolvable, undefined, resolveName).title, "Sent to qa-lat", "send title resolves pane id to tab name");
eq(renderTemplate(sentResolvable).title, "Sent to pane 45509574", "send title keeps id when no resolver");
const sentUnknown = recognize('codeterm send --pane deadbeef "yo"')!;
eq(renderTemplate(sentUnknown, undefined, resolveName).title, "Sent to pane deadbeef", "send title falls back to id when unresolved");

const spawn = recognize('codeterm agent spawn claude --task "do x" --workspace ws1')!;
eq(renderTemplate(spawn).title, "Spawned claude", "agent spawn title");

eq(renderTemplate(recognize("codeterm cron list --json")!).title, "cron list", "unknown → fallback title");

console.log("\n--- template paths ⊆ manifest ---\n");
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { TEMPLATE_PATHS } from "./template-paths";

const manifestPath = fileURLToPath(new URL("../../../../docs/cli-manifest.json", import.meta.url));
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));

function resolve(node: any, path: string[]): boolean {
  let cur = node;
  for (const seg of path) {
    cur = (cur.subcommands ?? []).find(
      (c: any) => c.name === seg || (c.aliases ?? []).includes(seg),
    );
    if (!cur) return false;
  }
  return true;
}

for (const path of TEMPLATE_PATHS) {
  ok(resolve(manifest, path), `template path exists in manifest: ${path.join(" ")}`);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
