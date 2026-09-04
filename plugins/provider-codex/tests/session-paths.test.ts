/**
 * Codex 0.153 session store tests. The index has no cwd, so detection must
 * use exact thread-history evidence and return null when that evidence is absent.
 * Run: npx tsx plugins/provider-codex/tests/session-paths.test.ts
 */

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

let passed = 0;
let failed = 0;

function assert(condition: boolean, name: string, detail?: string) {
  if (condition) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}${detail ? `: ${detail}` : ""}`); }
}

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  assert(a === e, name, `expected ${e}, got ${a}`);
}

function loadCodexWithHost(host: unknown) {
  const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
  const source = fs.readFileSync(pluginPath, "utf8");
  const context = { host, console, module: { exports: {} as { default?: any } }, exports: {} };
  new vm.Script(source, { filename: pluginPath }).runInNewContext(context);
  return context.module.exports.default;
}

const indexPath = "/tmp/home/.codex/session_index.jsonl";
const historyPath = "/tmp/home/.codex/thread_history_1.sqlite";
const projectSession = "01900000-0000-7000-8000-000000000001";
const foreignSession = "01900000-0000-7000-8000-000000000002";

const baseHost = (index: string, sqliteRows: (db: string, sql: string, params: unknown[]) => unknown[]) => {
  const files = new Map([[indexPath, index], [historyPath, "sqlite"]]);
  let sqliteCalls = 0;
  return {
    host: {
      manifestJson: () => "null",
      homeDir: () => "/tmp/home",
      platform: () => "linux",
      unixNowMs: () => Date.parse("2026-09-01T12:05:00Z"),
      path: { equal: (a: string, b: string) => a === b },
      expandHome: (v: string) => v.replace(/^~/, "/tmp/home"),
      envGet: (name: string) => name === "CODETERM_CODEX_SESSION_INDEX_PATH"
        ? indexPath
        : (name === "CODETERM_CODEX_THREAD_HISTORY_PATH" ? historyPath : null),
      log: () => {},
      shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
      fs: {
        fileExists: (file: string) => files.has(file),
        readFile: (file: string) => files.get(file) ?? null,
        sqliteQuery: (db: string, sql: string, params: unknown[]) => {
          sqliteCalls++;
          return sqliteRows(db, sql, params);
        },
        readFileHead: () => null,
        readFileTail: () => null,
        readJson: () => null,
        writeJson: () => true,
        writeFile: () => true,
        writeTempFile: () => "/tmp/codeterm-task.md",
        walkDir: () => [],
      },
    },
    sqliteCalls: () => sqliteCalls,
  };
};

const index = [
  JSON.stringify({ id: projectSession, thread_name: "new project prompt", updated_at: "2026-09-01T12:02:00Z" }),
  JSON.stringify({ id: foreignSession, thread_name: "foreign newer prompt", updated_at: "2026-09-01T12:04:00Z" }),
  JSON.stringify({ id: projectSession, thread_name: "older duplicate", updated_at: "2026-09-01T12:01:00Z" }),
].join("\n");

console.log("\n--- session_index folding and shared store paths ---\n");
{
  const fixture = baseHost(index, () => []);
  const p = loadCodexWithHost(fixture.host);
  const sessions = p.enumerateSessions();
  eq(sessions.length, 2, "repeated index ids fold to one session record");
  const folded = sessions.find((session: any) => session.sessionId === projectSession);
  eq(folded?.title, "new project prompt", "folding keeps the record with the newest updated_at");
  eq(folded?.filePath, indexPath, "enumeration reports the current session index store");
  eq(p.sessionFilePath("/work/proj", projectSession), null, "the new store has no JSONL session file");
  eq(p.sessionJsonlPath("/work/proj", projectSession), null, "chat path stays honest when no JSONL transcript exists");
}

console.log("\n--- detection refuses a cross-project recency guess ---\n");
{
  const fixture = baseHost(index, () => []);
  const p = loadCodexWithHost(fixture.host);
  eq(
    p.detectSessionId("/work/proj"),
    null,
    "missing SQLite cwd evidence returns null instead of binding the newer foreign session",
  );
  assert(fixture.sqliteCalls() > 0, "detection consults thread history before deciding unknown");
}

console.log("\n--- detection accepts one exact SQLite project match ---\n");
{
  const fixture = baseHost(index, (_db, _sql, params) => {
    const id = params[0];
    if (id === projectSession) {
      return [{ item_type: "thread_meta", item_json: JSON.stringify({ type: "thread_meta", payload: { cwd: "/work/proj" } }) }];
    }
    return [{ item_type: "thread_meta", item_json: JSON.stringify({ type: "thread_meta", payload: { cwd: "/other/proj" } }) }];
  });
  const p = loadCodexWithHost(fixture.host);
  eq(p.detectSessionId("/work/proj", [], 24 * 60 * 60 * 1000), projectSession, "exact SQLite cwd match wins over a newer foreign session");
  eq(p.detectSessionId("/work/proj", projectSession, 24 * 60 * 60 * 1000), null, "an excluded exact match leaves detection unknown");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
