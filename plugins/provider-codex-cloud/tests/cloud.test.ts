import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {test} from "node:test";
import vm from "node:vm";
import {cloudTaskBackend, cloudTaskSnapshot} from "../src/backend";
import {remoteChatBackend} from "../src/chatBackend";
import type {RemoteTaskBinding, RemoteTaskCapability, RemoteTaskStorage} from "../../../packages/plugin-sdk/src/remoteTasks";

const fixture = (name: string) => JSON.parse(readFileSync(new URL(`./fixtures/${name}.json`, import.meta.url), "utf8"));
const capability: RemoteTaskCapability = {
  backend: "chatgpt-cloud-tasks", localProviderId: "local-fixture", availability: {status: "available"},
  environments: [{id: "env-fixture", label: "Fixture environment", accountRef: "account-fixture"}], followUp: false, cancel: false, applyDiff: false,
};

function setup() {
  const saved = new Map<string, RemoteTaskBinding>();
  const storage: RemoteTaskStorage = {
    read: id => saved.has(id) ? JSON.parse(JSON.stringify(saved.get(id))) : null,
    compareExchange(id, revision, next) {
      if ((saved.get(id)?.revision ?? null) !== revision) return false;
      saved.set(id, JSON.parse(JSON.stringify(next))); return true;
    },
  };
  let details = fixture("task_details_with_diff");
  const calls: {method: string; path: string; account: string; body: unknown}[] = [];
  const backend = cloudTaskBackend(capability, {request(method, path, account, body) {
    calls.push({method, path, account, body});
    return method === "POST" ? {task: {id: "task_123"}} : details;
  }});
  const open = () => remoteChatBackend(storage, backend, "remote-fixture");
  return {storage, calls, open, setDetails(value: unknown) {details = value;}};
}

test("fake transport proves create shape, status/diff, restart binding, and explicit refresh", () => {
  const f = setup(); const plugin = f.open();
  const ctx = {tabId: "tab-fixture", config: {}, preset: "env-fixture"};
  plugin.openSession(ctx);
  plugin.sendMessage(ctx.tabId, "Read README title");
  assert.deepEqual(f.calls, [{method: "POST", path: "/wham/tasks", account: "account-fixture", body: {
    new_task: {environment_id: "env-fixture", branch: "master", run_environment_in_qa_mode: false},
    input_items: [{type: "message", role: "user", content: [{content_type: "text", text: "Read README title"}]}],
  }}]);
  const pending = plugin.poll(ctx.tabId);
  assert.match(pending.messages[0].content, /pending/);
  assert.equal(f.calls.length, 1, "transcript reads do not poll the remote server");
  const running = {...fixture("task_details_with_diff"), task_status_display: {latest_turn_status_display: {turn_status: "in_progress"}}};
  f.setDetails(running);
  plugin.viewCall("refresh", {sessionId: ctx.tabId});
  assert.equal(f.storage.read(ctx.tabId)?.snapshot?.status, "running");
  const completed = {...running, task_status_display: {latest_turn_status_display: {turn_status: "completed"}}};
  f.setDetails(completed);
  const restarted = f.open();
  restarted.openSession(ctx);
  const persisted = f.storage.read(ctx.tabId)!;
  assert.equal(persisted.snapshot?.status, "ready");
  assert.equal(persisted.snapshot?.result, "Assistant response");
  assert.match(persisted.snapshot?.diff ?? "", /diff --git/);
  assert.equal(f.calls.filter(call => call.method === "POST").length, 1);
  assert.throws(() => restarted.sendMessage(ctx.tabId, "follow up"));
  assert.throws(() => restarted.viewCall("cancel", {sessionId: ctx.tabId}));
  assert.throws(() => restarted.viewCall("apply", {sessionId: ctx.tabId}));
  const rendered = restarted.poll(ctx.tabId);
  assert.match(rendered.messages[0].content, /Assistant response/);
  assert.equal(restarted.poll(ctx.tabId, rendered.cursor).messages.length, 0);
});

test("source fixtures project nested diffs, errors, and successful no-diff tasks", () => {
  const f = setup(); const plugin = f.open();
  plugin.openSession({tabId: "tab-fixture", config: {}, preset: "env-fixture"});
  plugin.sendMessage("tab-fixture", "Read README");
  const binding = f.storage.read("tab-fixture")!;
  const failed = fixture("task_details_with_error");
  const error = cloudTaskSnapshot({...failed, task: {...failed.task, id: "task_123"}}, binding);
  assert.equal(error.status, "error");
  assert.equal(error.error, "Patch could not be applied");
  assert.match(error.diff ?? "", /diff --git/);
  const noDiff = cloudTaskSnapshot({task: {id: "task_123"}, current_assistant_turn: {turn_status: "completed", output_items: []}}, binding);
  assert.equal(noDiff.status, "ready"); assert.equal(noDiff.diff, null);
  assert.throws(() => cloudTaskSnapshot({task: {id: "foreign"}}, binding));
});

test("built production plugin refuses live access and never touches authentication", () => {
  const manifest = JSON.parse(readFileSync(new URL("../plugin.json", import.meta.url), "utf8"));
  const host = {
    manifestJson: () => JSON.stringify(manifest),
    fs: {expandHome: () => "/provider-state"},
  };
  const context = vm.createContext({module: {exports: {}}, host});
  vm.runInContext(readFileSync(new URL("../plugin.js", import.meta.url), "utf8"), context);
  const plugin = context.module.exports.default;
  assert.throws(() => plugin.openSession({tabId: "tab-fixture", config: {}}));
  assert.equal(manifest.capabilities.chatBackend.remoteTask.availability.status, "unavailable");
});

test("CLI starter task in session config is submitted once and never replayed on reopen", () => {
  const f = setup();
  const ctx = {tabId: "tab-fixture", config: {}, preset: "env-fixture", systemPrompt: "Read README title"};
  f.open().openSession(ctx);
  f.open().openSession(ctx);
  const creates = f.calls.filter(call => call.method === "POST");
  assert.equal(creates.length, 1);
  assert.equal((creates[0].body as {input_items: {content: {text: string}[]}[]}).input_items[0].content[0].text, ctx.systemPrompt);
  assert.equal(f.storage.read(ctx.tabId)?.snapshot?.taskId, "task_123");
});
