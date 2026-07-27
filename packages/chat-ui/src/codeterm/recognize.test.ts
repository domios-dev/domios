import { COMMAND_PATHS, type CommandPathNode } from "./commandPaths";
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
eq(inv?.flags.tab, "bda0e267", "send --pane flag");
eq(inv?.positionals, ["Понял, буду"], "send positional payload");

eq(recognize('codeterm agent spawn claude --task "go"')?.path, ["agent", "spawn"], "agent spawn path");
eq(recognize("CODETERM_API_PORT=7686 codeterm tab list")?.path, ["tab", "list"], "env-prefix stripped");
eq(recognize("cd frontend && codeterm agent report")?.path, ["agent", "report"], "cd-prefix chain strips to codeterm segment");
eq(
  recognize('cd /d/Developer/codeterm\ncodeterm agent report --status in_review --sha abc123 "done" 2>&1 | tail -3')?.path,
  ["agent", "report"],
  "Claude multiline report with output pipe",
);
eq(recognize("Set-Location 'D:\\Developer\\codeterm'; codeterm docs ORCHESTRATION")?.path, ["docs"], "PowerShell statement chain");
eq(
  recognize("env -u CODETERM_API_TOKEN CODETERM_API_PORT=7686 codeterm plan state pl_1")?.path,
  ["plan", "state"],
  "env wrapper stripped",
);
ok(recognize("~/.codeterm/bin/codeterm mem search foo") !== null, "abs path binary recognized");
ok(recognize("ls -la") === null, "non-codeterm → null");
ok(recognize('echo hi | codeterm send "x"') === null, "buried after pipe → null");
ok(recognize("codeterm-dev tab list") !== null, "codeterm-dev recognized");
ok(recognize("codeterm tabs") === null, "legacy tabs command stays unknown");
ok(recognize("codeterm tabs list") === null, "legacy tabs list stays unknown");
eq(recognize("codeterm cron list --json")?.path, ["cron", "list"], "known depth-2 subcommand");
eq(recognize("codeterm agent spawn claude")?.positionals, ["claude"], "trailing word stays positional");
eq(recognize("codeterm tab get-text --pane abc")?.path, ["tab", "get-text"], "tab get-text path");
eq(recognize("codeterm plan task update task_1 --status done")?.path, ["plan", "task", "update"], "depth-3 plan path");
eq(recognize("codeterm spec link add pl_1 --spec sp_1")?.path, ["spec", "link", "add"], "depth-3 spec path");

console.log("\n--- templates ---\n");
import { describeAction, renderTemplate } from "./templates";

const sent = recognize('codeterm send --pane bda0e267 "hi there"')!;
eq(renderTemplate(sent).title, "Sent to tab bda0e267", "send title");
eq(renderTemplate(sent, "OK (sent)").status, { ok: true, label: "delivered" }, "send status from result");

const sendAlias = recognize('codeterm send --pane keats "Привет от send"')!;
eq(renderTemplate(sendAlias).title, "Sent to tab keats", "send title from renamed command");
eq(renderTemplate(sendAlias, "OK").status, { ok: true, label: "delivered" }, "send status from result");

const resolveName = (ref: string) => (ref.slice(0, 8) === "45509574" ? "qa-lat" : undefined);
const sentResolvable = recognize('codeterm send --pane 45509574 "yo"')!;
eq(renderTemplate(sentResolvable, undefined, resolveName).title, "Sent to qa-lat", "send title resolves Tab id to tab name");
eq(renderTemplate(sentResolvable).title, "Sent to tab 45509574", "send title keeps id when no resolver");
const sentUnknown = recognize('codeterm send --pane deadbeef "yo"')!;
eq(renderTemplate(sentUnknown, undefined, resolveName).title, "Sent to tab deadbeef", "send title falls back to id when unresolved");

const spawn = recognize('codeterm agent spawn claude --task "do x" --workspace ws1')!;
eq(renderTemplate(spawn).title, "Spawned claude", "agent spawn title");
const close = recognize("codeterm tab close tab_8f1e2d")!;
eq(describeAction(close, undefined, (ref) => (ref === "tab_8f1e2d" ? "Research board" : undefined)).lead, "tab close", "tab close action lead");
eq(describeAction(close, undefined, (ref) => (ref === "tab_8f1e2d" ? "Research board" : undefined)).object, "Research board (tab_8f1e2d)", "tab close object includes closed id");
eq(describeAction(close).object, "tab tab_8f1e2d", "tab close fallback object uses raw id");

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

function manifestTree(node: any): CommandPathNode {
  return Object.fromEntries((node.subcommands ?? []).flatMap((child: any) => {
    const branch = manifestTree(child);
    return [[child.name, branch], ...(child.aliases ?? []).map((alias: string) => [alias, branch])];
  }));
}

eq(COMMAND_PATHS, manifestTree(manifest), "recognizer command tree matches manifest");

for (const path of TEMPLATE_PATHS) {
  ok(resolve(manifest, path), `template path exists in manifest: ${path.join(" ")}`);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
