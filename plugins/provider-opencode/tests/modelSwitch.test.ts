import assert from "node:assert/strict";
import { test } from "node:test";
import fs from "node:fs";
import vm from "node:vm";
import { DatabaseSync } from "node:sqlite";
import { opencodeBindingPluginSource } from "../src/helpers";

const controls = {
  id: "tab-owned", agent_session_id: "ses_owned", session_binding_nonce: "nonce",
  reported_session_binding_nonce: "nonce", model: "vendor/new/model", effort: "high",
};

async function binding(read: () => Promise<unknown>, sessionGet?: () => Promise<unknown>, envelope = (value: unknown): unknown => value, consumeReportSignal = false) {
  const gets: Array<{ url: string; headers: unknown }> = [];
  const warnings: unknown[] = [];
  const deadlines: number[] = [];
  const context = {
    process: { argv: [], cwd: () => "/work", env: {
      CODETERM_TAB_ID: "tab-owned", CODETERM_API_URL: "http://instance", CODETERM_API_TOKEN: "secret",
      CODETERM_SESSION_BINDING_NONCE: "nonce",
      CODETERM_RESUME_SESSION_ID: sessionGet ? "ses_owned" : undefined,
    } },
    AbortSignal: { timeout: (deadline: number) => { deadlines.push(deadline); return { aborted: false }; } },
    console: { warn: (message: unknown) => warnings.push(message) },
    fetch: async (url: string, options: { body?: string; headers: unknown; signal: {aborted: boolean} }) => {
      if (options.body) { if (consumeReportSignal) options.signal.aborted = true; return { ok: true }; }
      assert.equal(options.signal.aborted, false);
      gets.push({ url, headers: options.headers });
      assert.equal(url, "http://instance/api/tabs/tab-owned/turn-controls?session_id=ses_owned");
      return { ok: true, json: async () => envelope(await read()) };
    },
    module: { exports: undefined as unknown },
  };
  const source = opencodeBindingPluginSource().replace("export default async function", "async function");
  new vm.Script(`${source}\nmodule.exports = CodeTermSessionBindingPlugin;`).runInNewContext(context);
  const create = context.module.exports as (input: { directory: string; client?: { session: { get: () => Promise<unknown> } } }) => Promise<{
    "chat.message": (input: { sessionID: string }, output: { message: Record<string, unknown> }) => Promise<void>;
  }>;
  return { hooks: await create({ directory: "/work", client: sessionGet ? { session: { get: sessionGet } } : undefined }), gets, warnings, deadlines };
}

test("chat.message never waits for the startup lookup on OpenCode's own server", { timeout: 1000 }, async () => {
  const runtime = await binding(async () => controls, () => new Promise(() => {}));
  const message: Record<string, unknown> = {};
  await runtime.hooks["chat.message"]({ sessionID: "ses_owned" }, { message });
  assert.deepEqual(JSON.parse(JSON.stringify(message.model)), { providerID: "vendor", modelID: "new/model" });
});

test("typed and API prompts use exact-tab model and effort before message persistence", async () => {
  let current = { ...controls };
  const runtime = await binding(async () => current);
  const message: Record<string, unknown> = { model: { providerID: "old", modelID: "old" }, variant: "low" };
  await runtime.hooks["chat.message"]({ sessionID: "ses_owned" }, { message });
  assert.deepEqual(JSON.parse(JSON.stringify(message)), { model: { providerID: "vendor", modelID: "new/model" }, variant: "high" });
  current = { ...controls, model: "vendor/old", effort: "default" };
  await runtime.hooks["chat.message"]({ sessionID: "ses_owned" }, { message });
  assert.deepEqual(JSON.parse(JSON.stringify(message)), { model: { providerID: "vendor", modelID: "old" } });
  assert.deepEqual(JSON.parse(JSON.stringify(runtime.gets)), [0, 1].map(() => ({ url: "http://instance/api/tabs/tab-owned/turn-controls?session_id=ses_owned", headers: { authorization: "Bearer secret", "x-codeterm-binding-nonce": "nonce" } })));
  assert.deepEqual(runtime.deadlines, [200, 200, 200, 200]);
});

test("foreign tab, session, or stale binding cannot override a turn", async () => {
  for (const mismatch of [
    { id: "other" }, { agent_session_id: "ses_foreign" },
    { session_binding_nonce: "retired" }, { reported_session_binding_nonce: "retired" },
  ]) {
    const runtime = await binding(async () => ({ ...controls, ...mismatch }));
    const message = { model: "unchanged", variant: "unchanged" };
    await runtime.hooks["chat.message"]({ sessionID: "ses_owned" }, { message });
    assert.equal(runtime.warnings.length, 1);
    assert.deepEqual(message, { model: "unchanged", variant: "unchanged" });
  }
});

test("turn controls reject missing or unrelated envelopes", async () => {
  for (const envelope of [() => null, () => [], (value: unknown) => [value, value]]) {
    const runtime = await binding(async () => controls, undefined, envelope);
    const message = { model: "unchanged", variant: "unchanged" };
    await runtime.hooks["chat.message"]({ sessionID: "ses_owned" }, { message });
    assert.equal(runtime.warnings.length, 1);
    assert.deepEqual(message, { model: "unchanged", variant: "unchanged" });
  }
});

test("unavailable controls preserve the user message, warn once, and apply retained controls next turn", async () => {
  let available = false;
  const runtime = await binding(async () => {
    if (!available) throw new Error("timeout");
    return controls;
  });
  const message = { model: "unchanged", variant: "low", content: "opaque-user-input" };
  for (let i = 0; i < 2; i++) await runtime.hooks["chat.message"]({ sessionID: "ses_owned" }, { message });
  assert.deepEqual(message, { model: "unchanged", variant: "low", content: "opaque-user-input" });
  assert.equal(runtime.warnings.length, 1);
  available = true;
  await runtime.hooks["chat.message"]({ sessionID: "ses_owned" }, { message });
  assert.deepEqual(JSON.parse(JSON.stringify(message)), {
    model: { providerID: "vendor", modelID: "new/model" }, variant: "high", content: "opaque-user-input",
  });
  assert.equal(runtime.warnings.length, 1);
});

test("binding registration cannot consume the controls request deadline", async () => {
  const runtime = await binding(async () => controls, undefined, undefined, true);
  const message: Record<string, unknown> = {};
  await runtime.hooks["chat.message"]({ sessionID: "ses_owned" }, { message });
  assert.deepEqual(JSON.parse(JSON.stringify(message.model)), { providerID: "vendor", modelID: "new/model" });
});

test("confirmation reads first new assistant model in the claimed SQLite session", () => {
  const db = new DatabaseSync(":memory:");
  db.exec(fs.readFileSync(new URL("./fixtures/sqlite-1.18.35.sql", import.meta.url), "utf8"));
  const context = {
    host: { platform: () => "linux", envGet: () => null, expandHome: (p: string) => p,
      fs: { fileExists: () => true, sqliteQuery: (_path: string, sql: string, args: unknown[]) => {
        const statement = db.prepare(sql);
        statement.setAllowBareNamedParameters(true);
        return statement.all(Object.fromEntries(args.map((value, index) => [String(index + 1), value])));
      } },
    },
    module: { exports: {} as { default?: {
      sessionModelTranscriptBoundary: (cwd: string, sid: string) => unknown;
      detectSessionModelAfterBoundary: (cwd: string, sid: string, boundary: unknown) => string | null;
    } } }, exports: {},
  };
  new vm.Script(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8")).runInNewContext(context);
  const plugin = context.module.exports.default!;
  const boundary = plugin.sessionModelTranscriptBoundary("/fixture", "ses_fixture_owned");
  const detect = () => plugin.detectSessionModelAfterBoundary("/fixture", "ses_fixture_owned", boundary);
  const insert = (id: string, sid: string, time: number, message: unknown) => db.prepare(
    "INSERT INTO message VALUES (?, ?, ?, ?, ?)").run(id, sid, time, time, JSON.stringify(message));
  assert.equal(detect(), null);
  insert("msg_foreign_new", "ses_foreign", 3000, { role: "assistant", parentID: "foreign", providerID: "foreign", modelID: "foreign" });
  insert("msg_new_user", "ses_fixture_owned", 3000, { role: "user", model: { providerID: "vendor", modelID: "target" } });
  assert.equal(detect(), null);
  insert("msg_new_assistant", "ses_fixture_owned", 3001, { role: "assistant", parentID: "msg_new_user", providerID: "vendor", modelID: "target" });
  assert.equal(detect(), "vendor/target");
  insert("msg_later_assistant", "ses_fixture_owned", 4000, { role: "assistant", parentID: "later", providerID: "vendor", modelID: "later" });
  assert.equal(detect(), "vendor/target");
  db.close();
});

test("captured SQLite stores render owned text and tool history through the built reader", () => {
  for (const version of ["1.18.35", "1.19.0"]) {
    const db = new DatabaseSync(":memory:");
    db.exec(fs.readFileSync(new URL(`./fixtures/sqlite-${version}.sql`, import.meta.url), "utf8"));
    const context = {
      host: { platform: () => "linux", envGet: () => null, expandHome: (p: string) => p,
        fs: { fileExists: () => true, sqliteQuery: (_path: string, sql: string, args: unknown[]) => {
          const statement = db.prepare(sql);
          statement.setAllowBareNamedParameters(true);
          return statement.all(Object.fromEntries(args.map((value, index) => [String(index + 1), value])));
        } },
      },
      module: { exports: {} as { default?: {
        readStructuredChat: (cwd: string, sid: string, cursor?: string) => { messages: Array<{type: string; content: string}>; cursor: string };
      } } }, exports: {},
    };
    new vm.Script(fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8")).runInNewContext(context);
    const plugin = context.module.exports.default!;
    const chat = plugin.readStructuredChat("/fixture", "ses_fixture_owned");
    assert.deepEqual(Array.from(chat.messages, message => [message.type, message.content]).filter(([kind]) => kind === "user" || kind === "assistant").sort(), [
      ["assistant", "opaque-assistant-turn"], ["user", "opaque-user-turn"],
    ]);
    assert.equal(chat.messages.length, version === "1.19.0" ? 3 : 2);
    assert.equal(plugin.readStructuredChat("/fixture", "ses_fixture_owned", chat.cursor).messages.length, 0);
    assert.equal(plugin.readStructuredChat("/fixture", "ses_missing").messages.length, 0);
    db.close();
  }
});
