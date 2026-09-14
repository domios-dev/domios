/**
 * Codex 0.153 session store tests. The index has no cwd, so detection uses
 * exact state-thread evidence and returns null when that evidence is absent.
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
const stateDb = "/tmp/home/.codex/state_5.sqlite";
const overrideStateDb = "/tmp/home/.codex/state_override.sqlite";
const projectCwd = "/work/proj";
const foreignCwd = "/other/proj";
const missingCwd = "/gone/proj";
const projectSession = "01900000-0000-7000-8000-000000000001";
const foreignSession = "01900000-0000-7000-8000-000000000002";
const noRowSession = "01900000-0000-7000-8000-000000000003";
const noCwdSession = "01900000-0000-7000-8000-000000000004";

type Query = { db: string; sql: string; params: unknown[] };

const baseHost = (
  index: string,
  sqliteRows: (db: string, sql: string, params: unknown[]) => unknown[],
  options?: { stateOverride?: string | null; existing?: string[] },
) => {
  const existing = new Set([
    indexPath,
    stateDb,
    projectCwd,
    foreignCwd,
    ...(options?.existing || []),
  ]);
  const files = new Map([[indexPath, index]]);
  const queries: Query[] = [];
  const logs: { level: string; message: string }[] = [];
  const fileChecks: string[] = [];
  let readDirCalls = 0;
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
        : (name === "CODETERM_CODEX_STATE_DB_PATH" ? (options?.stateOverride || null) : null),
      log: (level: string, message: string) => logs.push({ level, message }),
      shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
      fs: {
        fileExists: (file: string) => {
          fileChecks.push(file);
          return existing.has(file) || files.has(file);
        },
        readFile: (file: string) => files.get(file) ?? null,
        sqliteQuery: (db: string, sql: string, params: unknown[]) => {
          queries.push({ db, sql, params });
          return sqliteRows(db, sql, params);
        },
        readDir: () => {
          readDirCalls++;
          return [
            { name: "state_4.sqlite", isFile: true },
            { name: "state_5.sqlite", isFile: true },
            { name: "sessions", isFile: false },
          ];
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
    queries,
    logs,
    fileChecks,
    readDirCalls: () => readDirCalls,
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
  eq(p.sessionFilePath(projectCwd, projectSession), null, "a thread with no state row names no transcript");
  eq(p.sessionJsonlPath(projectCwd, projectSession), null, "chat path stays honest when no JSONL transcript exists");
  eq(p.sessionExists?.(projectCwd, projectSession), true, "claimed indexed session exists without a JSONL path");
  eq(p.sessionExists?.(projectCwd, "unknown-session"), false, "another indexed session cannot prove the claimed session");
  eq(p.sessionExists?.(projectCwd, ""), false, "empty session claims are refused");
}

console.log("\n--- thread_cwd_from_state_row_drives_detection_and_enumeration ---\n");
{
  const fixture = baseHost(index, (_db, sql, params) => {
    if (!/FROM threads/.test(sql)) return [];
    return params[0] === projectSession ? [{ cwd: projectCwd }] : [{ cwd: foreignCwd }];
  });
  const p = loadCodexWithHost(fixture.host);
  eq(p.detectSessionId(projectCwd), projectSession,
    "the exact existing thread cwd selects its own indexed session");
  eq(p.detectSessionId(projectCwd, projectSession, 24 * 60 * 60 * 1000), null,
    "an excluded exact match leaves detection unknown");
  const sessions = p.enumerateSessions();
  eq(sessions.find((session: any) => session.sessionId === projectSession)?.projectPath, projectCwd,
    "enumeration reports the matching thread cwd");
  eq(sessions.find((session: any) => session.sessionId === foreignSession)?.projectPath, foreignCwd,
    "enumeration keeps a foreign row on its own cwd");
  const threadQueries = fixture.queries.filter((query) => /FROM threads/.test(query.sql));
  assert(threadQueries.length >= 4, "both consumers query the shared thread row");
  assert(threadQueries.every((query) => query.sql === "SELECT model, rollout_path, cwd FROM threads WHERE id = ? LIMIT 1"),
    "cwd uses the exact threads query shape");
  assert(threadQueries.every((query) => !query.sql.includes("thread_items")),
    "cwd detection never queries thread_items");
  assert(threadQueries.every((query) => query.params[0] === projectSession || query.params[0] === foreignSession),
    "each query is scoped to an indexed session id");
  assert(fixture.fileChecks.includes(projectCwd) && fixture.fileChecks.includes(foreignCwd),
    "each returned cwd is validated through the host filesystem");
  assert(fixture.logs.length === 0, "resolved rows do not emit unresolved diagnostics");
}

console.log("\n--- ambiguous_thread_cwds_stay_unknown ---\n");
{
  const ambiguousIndex = [
    JSON.stringify({ id: projectSession, thread_name: "first", updated_at: "2026-09-01T12:02:00Z" }),
    JSON.stringify({ id: foreignSession, thread_name: "second", updated_at: "2026-09-01T12:04:00Z" }),
  ].join("\n");
  const fixture = baseHost(ambiguousIndex, () => [{ cwd: projectCwd }]);
  const p = loadCodexWithHost(fixture.host);
  eq(p.detectSessionId(projectCwd, [], 24 * 60 * 60 * 1000), null,
    "ambiguous exact thread cwd evidence stays unknown");
  eq(fixture.logs.length, 0, "resolved ambiguity does not emit an unresolved diagnostic");
}

console.log("\n--- missing_thread_cwd_stays_unknown_without_neighbour_borrow ---\n");
{
  const noEvidenceIndex = [
    JSON.stringify({ id: projectSession, thread_name: "project", updated_at: "2026-09-01T12:02:00Z" }),
    JSON.stringify({ id: foreignSession, thread_name: "foreign", updated_at: "2026-09-01T12:04:00Z" }),
    JSON.stringify({ id: noCwdSession, thread_name: "blank cwd", updated_at: "2026-09-01T12:03:00Z" }),
  ].join("\n");
  const fixture = baseHost(noEvidenceIndex, (_db, _sql, params) => {
    if (params[0] === foreignSession) return [{ cwd: foreignCwd }];
    if (params[0] === noCwdSession) return [{ cwd: "   " }];
    return [];
  });
  const p = loadCodexWithHost(fixture.host);
  eq(p.detectSessionId(projectCwd, [], 24 * 60 * 60 * 1000), null,
    "a missing own row stays unknown instead of borrowing a foreign cwd");
  eq(p.detectSessionId(projectCwd, [], 24 * 60 * 60 * 1000), null,
    "repeated missing-row detection remains unknown");
  const sessions = p.enumerateSessions();
  eq(sessions.find((session: any) => session.sessionId === projectSession)?.projectPath, null,
    "a missing thread row leaves projectPath unknown");
  eq(sessions.find((session: any) => session.sessionId === noCwdSession)?.projectPath, null,
    "a blank thread cwd leaves projectPath unknown");
  const diagnostics = fixture.logs.map((entry) => JSON.parse(entry.message));
  eq(diagnostics.length, 2, "each unresolved session emits one diagnostic");
  eq(diagnostics.filter((entry) => entry.sessionId === projectSession).length, 1,
    "the missing-row diagnostic is deduplicated per session");
  eq(diagnostics.filter((entry) => entry.sessionId === noCwdSession).length, 1,
    "the missing-cwd diagnostic is deduplicated per session");
  eq(diagnostics.find((entry) => entry.sessionId === projectSession)?.cause, "no_row",
    "the missing-row diagnostic names its cause");
  eq(diagnostics.find((entry) => entry.sessionId === noCwdSession)?.cause, "no_cwd",
    "the missing-cwd diagnostic names its cause");
  eq(diagnostics.filter((entry) => entry.sessionId === foreignSession).length, 0,
    "the foreign row needs no unresolved diagnostic");
}

console.log("\n--- nonexistent_thread_cwd_is_absent_and_diagnostic_deduplicates ---\n");
{
  const missingIndex = JSON.stringify({ id: projectSession, thread_name: "missing", updated_at: "2026-09-01T12:02:00Z" });
  const fixture = baseHost(missingIndex, (_db, _sql, params) =>
    params[0] === projectSession ? [{ cwd: missingCwd }] : []);
  const p = loadCodexWithHost(fixture.host);
  eq(p.detectSessionId(projectCwd), null, "a missing-on-disk thread cwd cannot bind detection");
  eq(p.enumerateSessions()[0]?.projectPath, null, "a missing-on-disk thread cwd is not enumerated");
  eq(p.detectSessionId(projectCwd), null, "repeated missing-path detection remains unknown");
  eq(p.enumerateSessions()[0]?.projectPath, null, "repeated enumeration remains unknown");
  const diagnostics = fixture.logs.map((entry) => JSON.parse(entry.message));
  eq(diagnostics.length, 1, "the missing-path diagnostic is emitted once");
  eq(diagnostics[0].provider, "codex", "the diagnostic identifies the provider");
  eq(diagnostics[0].event, "session_cwd_unresolved", "the diagnostic identifies the event");
  eq(diagnostics[0].sessionId, projectSession, "the missing-path diagnostic names the session");
  eq(diagnostics[0].cause, "cwd_absent", "the missing-path diagnostic names cwd_absent");
  assert(fixture.fileChecks.includes(missingCwd), "the recorded cwd is checked for on-disk existence");
}

console.log("\n--- thread_row_reads_model_rollout_and_cwd_from_highest_state_db ---\n");
{
  const rollout = "/tmp/home/.codex/sessions/2026/09/01/rollout-2026-09-01T15-02-00-" + projectSession + ".jsonl";
  const fixture = baseHost(index, (_db, _sql, params) =>
    params[0] === projectSession
      ? [{ model: "gpt-5.6-luna", rollout_path: rollout, cwd: projectCwd }]
      : [{ model: "gpt-5.5", rollout_path: "/tmp/foreign.jsonl", cwd: foreignCwd }],
    { existing: [rollout, "/tmp/foreign.jsonl"] });
  const p = loadCodexWithHost(fixture.host);

  eq(p.detectSessionModel(projectCwd, projectSession), "gpt-5.6-luna",
    "the running model is read from the thread row Codex maintains");
  eq(p.sessionFilePath(projectCwd, projectSession), rollout,
    "the rollout the session appends to is a real file the usage parser can read");
  eq(p.detectSessionId(projectCwd, [], 24 * 60 * 60 * 1000), projectSession,
    "thread cwd remains usable alongside model and rollout fields");
  eq(p.enumerateSessions().find((session: any) => session.sessionId === projectSession)?.projectPath, projectCwd,
    "enumeration exposes the same existing thread cwd");

  const threadQueries = fixture.queries.filter((query) => /FROM threads/.test(query.sql));
  assert(threadQueries.length > 0, "all session fields use the thread row");
  assert(threadQueries.every((query) => query.db === stateDb),
    "the newest state database wins, so a version bump does not silently stop answering");
  assert(threadQueries.every((query) => query.sql === "SELECT model, rollout_path, cwd FROM threads WHERE id = ? LIMIT 1"),
    "model, rollout, and cwd share one exact query");
  assert(threadQueries.every((query) => !query.sql.includes("thread_items")),
    "the item table is not consulted for session fields");
  assert(threadQueries.every((query) => query.params[0] === projectSession || query.params[0] === foreignSession),
    "each thread query is scoped to its own session id");

  const overrideFixture = baseHost(index, (_db, _sql, params) =>
    params[0] === projectSession ? [{ model: "gpt-5.6-luna", rollout_path: rollout, cwd: projectCwd }] : [],
    { stateOverride: overrideStateDb, existing: [overrideStateDb, rollout] });
  const overridePlugin = loadCodexWithHost(overrideFixture.host);
  eq(overridePlugin.detectSessionModel(projectCwd, projectSession), "gpt-5.6-luna",
    "the explicit state database override still serves model reads");
  eq(overridePlugin.sessionFilePath(projectCwd, projectSession), rollout,
    "the explicit state database override still serves rollout reads");
  eq(overridePlugin.detectSessionId(projectCwd), projectSession,
    "the explicit state database override still serves cwd detection");
  const overrideQueries = overrideFixture.queries.filter((query) => /FROM threads/.test(query.sql));
  assert(overrideQueries.every((query) => query.db === overrideStateDb),
    "the explicit state database override wins over discovery");
  eq(overrideFixture.readDirCalls(), 0, "an explicit state database avoids discovery");
}

console.log("\n--- an absent rollout is reported, not handed to the parser ---\n");
{
  const fixture = baseHost(index, (_db, sql) =>
    /FROM threads/.test(sql) ? [{ model: "gpt-5.6-luna", rollout_path: "/tmp/gone.jsonl", cwd: projectCwd }] : []);
  const p = loadCodexWithHost(fixture.host);
  eq(p.sessionFilePath(projectCwd, projectSession), null,
    "a rollout path that is not on disk is reported as no transcript");
  eq(p.detectSessionModel(projectCwd, projectSession), "gpt-5.6-luna",
    "the model stays known even when the transcript file has been cleaned up");
}

console.log("\n--- an empty session index stays an ordinary empty result ---\n");
{
  const fixture = baseHost("", () => []);
  const p = loadCodexWithHost(fixture.host);
  eq(p.enumerateSessions(), [], "an empty session index enumerates no sessions");
  eq(p.detectSessionId(projectCwd), null, "an empty session index detects no session");
  eq(fixture.logs.length, 0, "an empty session index emits no unresolved diagnostic");
}

console.log("\n--- detection rejects an absent requested cwd ---\n");
{
  const fixture = baseHost(index, (_db, _sql, params) =>
    params[0] === projectSession ? [{ cwd: projectCwd }] : [{ cwd: foreignCwd }]);
  const p = loadCodexWithHost(fixture.host);
  eq(p.detectSessionId(missingCwd), null, "an absent requested cwd is never bound");
  eq(fixture.queries.length, 0, "an absent requested cwd avoids unrelated state-row reads");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
