import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {test} from "node:test";
import vm from "node:vm";
import {CLOUD_SIGN_IN_REASON, CODEX_CLOUD_ACCOUNT_REF, cloudTaskBackend, cloudTaskSnapshot} from "../src/cloud/backend";
import {remoteChatBackend} from "../src/cloud/chatBackend";
import {cloudExports} from "../src/cloud/plugin";
import {createCodexCloudTransport} from "../src/cloud/transport";
import type {Host, ViewCallContext} from "@codeterm/plugin-sdk";
import type {RemoteTaskBinding, RemoteTaskCapability, RemoteTaskStorage} from "../../../packages/plugin-sdk/src/remoteTasks";

const VIEW_CONTEXT: ViewCallContext = {
  plugin: "codex", origin: "remote_task", surface: null, instance: null,
  tab: "tab-fixture", owner: "tab-fixture", workspace: "workspace-fixture", team: null,
  actor: {kind: "local"}, frame: null,
};

const fixture = (name: string) => JSON.parse(readFileSync(new URL(`./cloud-fixtures/${name}.json`, import.meta.url), "utf8"));
const capability: RemoteTaskCapability = {
  backend: "chatgpt-cloud-tasks", mode: "cloud", displayName: "Cloud", availability: {status: "available"},
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
  const backend = cloudTaskBackend(capability, {discoverRemoteTask: () => ({availability: {status: "available"}, environments: capability.environments}), request(method, path, account, body) {
    calls.push({method, path, account, body});
    return method === "POST" ? {task: {id: "task_123"}} : details;
  }});
  const open = () => remoteChatBackend(storage, backend, "remote-fixture");
  return {storage, calls, open, setDetails(value: unknown) {details = value;}};
}

test("fake transport proves create shape, status/diff, restart binding, and explicit refresh", () => {
  const f = setup(); const plugin = f.open();
  const ctx = {tabId: "tab-fixture", config: {executionMode: "cloud"}, preset: "env-fixture"};
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
  plugin.viewCall("refresh", {sessionId: ctx.tabId}, VIEW_CONTEXT);
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
  assert.ok(f.calls.some(call => call.method === "GET" && call.path === "/wham/tasks/task_123"));
  assert.throws(() => restarted.sendMessage(ctx.tabId, "follow up"));
  assert.throws(() => restarted.viewCall("cancel", {sessionId: ctx.tabId}, VIEW_CONTEXT));
  assert.throws(() => restarted.viewCall("apply", {sessionId: ctx.tabId}, VIEW_CONTEXT));
  const rendered = restarted.poll(ctx.tabId);
  assert.match(rendered.messages[0].content, /Assistant response/);
  assert.equal(restarted.poll(ctx.tabId, rendered.cursor).messages.length, 0);
});

test("Settings environment IDs use the Codex task API and retain the environment_id field", () => {
  const environment = {id: "asenvcfg_redacted-one", label: "domios 1", accountRef: CODEX_CLOUD_ACCOUNT_REF};
  const currentCapability = {...capability, environments: [environment]};
  const calls: Array<{method: string; path: string; body: unknown}> = [];
  const backend = cloudTaskBackend(currentCapability, {
    discoverRemoteTask: () => ({availability: {status: "available"}, environments: [environment]}),
    request(method, path, _account, body) {
      calls.push({method, path, body});
      return method === "POST" ? {task: {id: "task-redacted"}} : {...fixture("task_details_with_diff"), task: {id: "task-redacted"}};
    },
  });
  const created = backend.create({environmentId: environment.id, accountRef: environment.accountRef, branch: "master", prompt: "Read-only verification"});
  backend.read({
    revision: 1, providerId: "codex", executionMode: "cloud", backend: currentCapability.backend,
    environmentId: environment.id, accountRef: environment.accountRef, branch: "master", phase: "bound",
    snapshot: {taskId: created.taskId, attempt: null, status: "pending", result: "", diff: null, cursor: null, error: null},
  });
  assert.deepEqual(calls, [
    {
      method: "POST", path: "/api/codex/tasks", body: {
        new_task: {environment_id: "asenvcfg_redacted-one", branch: "master", run_environment_in_qa_mode: false},
        input_items: [{type: "message", role: "user", content: [{content_type: "text", text: "Read-only verification"}]}],
      },
    },
    {method: "GET", path: "/api/codex/tasks/task-redacted", body: undefined},
  ]);
});

test("source fixtures project nested diffs, errors, and successful no-diff tasks", () => {
  const f = setup(); const plugin = f.open();
  plugin.openSession({tabId: "tab-fixture", config: {executionMode: "cloud"}, preset: "env-fixture"});
  plugin.sendMessage("tab-fixture", "Read README");
  const binding = f.storage.read("tab-fixture")!;
  const failed = fixture("task_details_with_error");
  const error = cloudTaskSnapshot({...failed, task: {...failed.task, id: "task_123"}}, binding);
  assert.equal(error.status, "error");
  assert.equal(error.error, "Remote task failed");
  assert.match(error.diff ?? "", /diff --git/);
  const noDiff = cloudTaskSnapshot({task: {id: "task_123"}, current_assistant_turn: {turn_status: "completed", output_items: []}}, binding);
  assert.equal(noDiff.status, "ready"); assert.equal(noDiff.diff, null);
  assert.throws(() => cloudTaskSnapshot({task: {id: "foreign"}}, binding));
});

test("Codex transport discovers Settings environments through host credentials without persisting account identity", () => {
  const calls: Array<{url: string; method: string; headers: Record<string, string>; credential: unknown}> = [];
  const host = {
    credentialPublic(id: string) {
      assert.equal(id, "codexAuth");
      return JSON.stringify({accountId: "redacted-account-fixture"});
    },
    fetch(raw: string) {
      const request = JSON.parse(raw) as {url: string; method: string; headers: Record<string, string>; credential: unknown};
      calls.push(request);
      const name = request.url.includes("environment-configs") ? "environment_configs" : request.url.includes("/by-repo/") ? "repo_environments" : "environments";
      return JSON.stringify({status: 200, body: JSON.stringify(fixture(name))});
    },
  } as unknown as Host;

  const discovery = createCodexCloudTransport(host).discoverRemoteTask();
  assert.deepEqual(discovery, {
    availability: {status: "available"},
    environments: [
      {id: "asenvcfg_redacted-one", label: "domios 1", accountRef: CODEX_CLOUD_ACCOUNT_REF},
      {id: "asenvcfg_redacted-two", label: "domios 2", accountRef: CODEX_CLOUD_ACCOUNT_REF},
    ],
  });
  assert.deepEqual(calls.map(call => [call.method, `${new URL(call.url).pathname}${new URL(call.url).search}`]), [
    ["GET", "/api/codex-cloud/v1/environment-configs?scope=user&limit=100&omitDraft=true"],
  ]);
  assert.equal(calls[0].headers["ChatGPT-Account-Id"], "redacted-account-fixture");
  assert.deepEqual(calls[0].credential, {id: "codexAuth", headers: {Authorization: "Bearer {accessToken}"}});
  assert.equal(discovery.environments.some(environment => environment.accountRef === "redacted-account-fixture"), false);
  assert.equal(discovery.environments.some(environment => environment.id.startsWith("cecfgver_")), false);
});

test("Codex transport falls back to the account environment list and redacts expiry and fetch errors", () => {
  const paths: string[] = [];
  const host = {
    credentialPublic: () => JSON.stringify({accountId: "redacted-account-fixture"}),
    fetch(raw: string) {
      const request = JSON.parse(raw) as {url: string};
      paths.push(new URL(request.url).pathname);
      if (paths.length === 1) return JSON.stringify({status: 404, body: "not found"});
      return JSON.stringify({status: 200, body: JSON.stringify(fixture("environments"))});
    },
  } as unknown as Host;
  const transport = createCodexCloudTransport(host);
  assert.deepEqual(transport.discoverRemoteTask().environments.map(environment => environment.label), ["domios 1", "domios 2"]);
  assert.deepEqual(paths, [
    "/api/codex-cloud/v1/environment-configs",
    "/backend-api/wham/environments/by-repo/github/rollacode/codeterm",
    "/backend-api/wham/environments",
  ]);

  const expired = createCodexCloudTransport({
    credentialPublic: () => null,
    fetch: () => { throw new Error("secret-token-never-report"); },
  } as unknown as Host).discoverRemoteTask();
  assert.deepEqual(expired, {availability: {status: "unavailable", reason: CLOUD_SIGN_IN_REASON}, environments: []});

  const expiredToken = createCodexCloudTransport({
    credentialPublic: () => JSON.stringify({accountId: "redacted-account-fixture"}),
    fetch: () => JSON.stringify({status: 401, body: "Bearer secret-token-never-report"}),
  } as unknown as Host).discoverRemoteTask();
  assert.deepEqual(expiredToken, {availability: {status: "unavailable", reason: CLOUD_SIGN_IN_REASON}, environments: []});

  const failed = createCodexCloudTransport({
    credentialPublic: () => JSON.stringify({accountId: "redacted-account-fixture"}),
    fetch: () => { throw new Error("secret-token-never-report"); },
  } as unknown as Host);
  assert.throws(() => failed.request("GET", "/wham/tasks/list", CODEX_CLOUD_ACCOUNT_REF), error => {
    assert.equal((error as Error).message, "Codex Cloud request failed.");
    assert.equal((error as Error).message.includes("secret-token-never-report"), false);
    return true;
  });
});

test("Codex transport uses the read-only task list route", () => {
  let requestedPath = "";
  const host = {
    credentialPublic: () => JSON.stringify({accountId: "redacted-account-fixture"}),
    fetch(raw: string) {
      const url = new URL((JSON.parse(raw) as {url: string}).url);
      requestedPath = `${url.pathname}${url.search}`;
      return JSON.stringify({status: 200, body: JSON.stringify(fixture("tasks_list"))});
    },
  } as unknown as Host;
  const tasks = createCodexCloudTransport(host).request("GET", "/wham/tasks/list?limit=10", CODEX_CLOUD_ACCOUNT_REF);
  assert.equal(requestedPath, "/backend-api/wham/tasks/list?limit=10");
  assert.deepEqual(tasks, fixture("tasks_list"));

  const currentTasks = createCodexCloudTransport(host).request("GET", "/api/codex/tasks/list?limit=100", CODEX_CLOUD_ACCOUNT_REF);
  assert.equal(requestedPath, "/api/codex/tasks/list?limit=100");
  assert.deepEqual(currentTasks, fixture("tasks_list"));
  const transport = createCodexCloudTransport(host);
  assert.throws(() => transport.request("POST", "/api/codex/tasks/list", CODEX_CLOUD_ACCOUNT_REF));
  assert.throws(() => transport.request("GET", "/api/codex/tasks", CODEX_CLOUD_ACCOUNT_REF));
});

test("built production plugin shows a safe sign-in reason without an account", () => {
  const manifest = JSON.parse(readFileSync(new URL("../plugin.json", import.meta.url), "utf8"));
  const host = {
    manifestJson: () => JSON.stringify(manifest),
    dataDir: () => "/provider-state",
    fs: {expandHome: () => { throw new Error("Cloud storage must use the instance data root"); }},
    credentialPublic: () => null,
    fetch: () => { throw new Error("Unexpected unauthenticated request"); },
  };
  const context = vm.createContext({module: {exports: {}}, host});
  vm.runInContext(readFileSync(new URL("../plugin.js", import.meta.url), "utf8"), context);
  const plugin = context.module.exports.default;
  assert.throws(() => plugin.openSession({tabId: "tab-fixture", config: {executionMode: "cloud"}}), /Sign in to Codex/);
  assert.equal(manifest.id, "codex");
  assert.equal(manifest.capabilities.remoteTask.availability.status, "unavailable");
  assert.equal(manifest.capabilities.agentRuntime.runtime, "stdio-jsonrpc");
  assert.equal(typeof plugin.runtimePlan, "function");
  assert.equal(typeof plugin.runtimeReduce, "function");
});

test("CLI starter task in session config is submitted once and never replayed on reopen", () => {
  const f = setup();
  const ctx = {tabId: "tab-fixture", config: {executionMode: "cloud"}, preset: "env-fixture", systemPrompt: "Read README title"};
  f.open().openSession(ctx);
  f.open().openSession(ctx);
  const creates = f.calls.filter(call => call.method === "POST");
  assert.equal(creates.length, 1);
  assert.equal((creates[0].body as {input_items: {content: {text: string}[]}[]}).input_items[0].content[0].text, ctx.systemPrompt);
  assert.equal(f.storage.read(ctx.tabId)?.snapshot?.taskId, "task_123");
});

test("the same provider refuses Local and unsupported modes before any Cloud request", () => {
  const f = setup();
  for (const config of [{}, {executionMode: "local"}, {executionMode: "foreign"}]) {
    assert.throws(() => f.open().openSession({tabId: "tab-fixture", config, preset: "env-fixture", systemPrompt: "Never submit"}));
  }
  assert.equal(f.calls.length, 0);
  assert.equal(f.storage.read("tab-fixture"), null);
});

test("a saved task from a foreign provider refuses restore before remote reads", () => {
  const f = setup();
  const ctx = {tabId: "tab-fixture", config: {executionMode: "cloud"}, preset: "env-fixture"};
  f.open().openSession(ctx);
  f.open().openSession(ctx);
  const plugin = f.open(); plugin.openSession(ctx); plugin.sendMessage(ctx.tabId, "Read README");
  const saved = f.storage.read(ctx.tabId)!;
  f.storage.compareExchange(ctx.tabId, saved.revision, {...saved, providerId: "foreign-provider", revision: saved.revision + 1});
  const calls = f.calls.length;
  assert.throws(() => f.open().openSession(ctx));
  assert.equal(f.calls.length, calls);
});


test("Cloud task storage uses the supplied instance data root", () => {
  for (const root of ["/custom/domios-named/", "C:\\Users\\Owner\\.domios-dev-fixture\\"]) {
    let readPath = "";
    const host = {
      dataDir: () => root,
      manifestJson: () => JSON.stringify({id: "codex", capabilities: {remoteTask: capability}}),
      credentialPublic: () => null,
      fs: {
        expandHome: () => { throw new Error("Legacy root resolution is forbidden"); },
        readFile(path: string) { readPath = path; return null; },
      },
    } as unknown as Host;
    cloudExports(() => host).viewCall!("read", {sessionId: "tab-fixture"}, VIEW_CONTEXT);
    assert.equal(readPath, `${root.replace(/[\\/]$/, "")}/codex/tasks/tab-fixture.json`);
  }
});

test("Cloud task storage reports an unavailable instance root", () => {
  for (const root of [null, undefined, ""]) {
    const host = {
      dataDir: () => root,
      manifestJson: () => JSON.stringify({id: "codex", capabilities: {remoteTask: capability}}),
    } as unknown as Host;
    assert.throws(() => cloudExports(() => host).viewCall!("read", {sessionId: "tab-fixture"}, VIEW_CONTEXT), /Cloud task storage is unavailable/);
  }
});
