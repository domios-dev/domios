// Run: npx tsx plugins/provider-opencode/tests/lifecycle.test.ts

import assert from "node:assert/strict";
import { test } from "node:test";
import vm from "node:vm";
import fs from "node:fs";
import { opencodeBindingPluginSource } from "../src/helpers";

type Event = { type: string; properties?: Record<string, unknown> };
type Hooks = { event: (input: { event: Event }) => Promise<void> };

test("every_launch_shape_scopes_the_resume_session_to_the_worker_environment", () => {
  const source = fs.readFileSync(new URL("../plugin.js", import.meta.url), "utf8");
  for (const platform of ["windows", "linux"]) {
    const quoteFor = (value: string) => `'${value}'`;
    const context = {
      host: {
        platform: () => platform,
        envGet: () => null,
        expandHome: (value: string) => value,
        shell: { quoteFor },
        fs: { writeTempFile: () => "/opaque-instructions.md", fileExists: () => false },
      },
      module: { exports: {} as { default?: any } }, exports: {},
    };
    new vm.Script(source).runInNewContext(context);
    const plugin = context.module.exports.default;
    const assignment = platform === "windows" ? "$env:CODETERM_RESUME_SESSION_ID = " : "CODETERM_RESUME_SESSION_ID=";
    for (const command of [
      plugin.buildLaunchCommand({ sessionId: "ses_owned" }),
      plugin.buildResumeCommand("ses_owned"),
      plugin.buildResumeCommandWithContext({ sessionId: "ses_owned", systemPrompt: "opaque" }),
    ]) assert.ok(command.includes(assignment + "'ses_owned'"));
    assert.ok(plugin.buildLaunchCommand({}).includes(assignment + "''"));
  }
});

async function loadBinding(input: {
  argv?: string[];
  resumeSession?: string;
  waitForStartup?: boolean;
  bindingNonce?: string;
  sessionGet?: (args: unknown) => Promise<unknown>;
} = {}) {
  const posts: Array<{ url: string; body: Record<string, unknown>; headers: Record<string, string> }> = [];
  const source = opencodeBindingPluginSource().replace(
    "export default async function CodeTermSessionBindingPlugin",
    "async function CodeTermSessionBindingPlugin",
  );
  const context = {
    AbortSignal,
    process: {
      argv: input.argv || [],
      env: {
        CODETERM_TAB_ID: "tab-1",
        CODETERM_API_TOKEN: "token",
        CODETERM_SESSION_BINDING_NONCE: input.bindingNonce || "nonce",
        CODETERM_API_URL: "http://dev",
        CODETERM_RESUME_SESSION_ID: input.resumeSession,
      },
      cwd: () => "/work",
    },
    fetch: async (url: string, options: { body: string; headers: Record<string, string> }) => {
      posts.push({ url, body: JSON.parse(options.body) as Record<string, unknown>, headers: options.headers });
      return { ok: true };
    },
    module: { exports: undefined as unknown },
  };
  new vm.Script(`${source}\nmodule.exports = CodeTermSessionBindingPlugin;`).runInNewContext(context);
  const makePlugin = context.module.exports as (value: {
    directory: string;
    client?: { session?: { get?: (args: unknown) => Promise<unknown> } };
  }) => Promise<Hooks>;
  const hooks = await makePlugin({
    directory: "/work",
    client: input.sessionGet ? { session: { get: input.sessionGet } } : undefined,
  });
  if (input.waitForStartup !== false) await hooks.event({ event: { type: "test.startup.completed" } });
  return { hooks, posts };
}

test("resume_factory_returns_before_own_server_session_lookup_can_complete", async () => {
  let completeLookup!: (value: unknown) => void;
  const lookup = new Promise((resolve) => { completeLookup = resolve; });
  const binding = await loadBinding({
    resumeSession: "ses_owned",
    waitForStartup: false,
    bindingNonce: "current-launch-nonce",
    sessionGet: () => lookup,
  });
  assert.deepEqual(binding.posts, []);
  completeLookup({ data: { id: "ses_owned", directory: "/work" } });
  await binding.hooks.event({ event: { type: "test.startup.completed" } });
  assert.deepEqual(binding.posts.map(({ body }) => [body.session_id, body.source]), [["ses_owned", "session.get"]]);
  assert.equal(binding.posts[0].headers["x-codeterm-binding-nonce"], "current-launch-nonce");
  assert.equal(binding.posts[0].url, "http://dev/api/tabs/tab-1/hook/session_start");
});

test("opencode_resume_rebinds_session", async () => {
  const requests: unknown[] = [];
  const resumed = await loadBinding({
    argv: ["--session", "ses_resumed"],
    sessionGet: async (args) => {
      requests.push(args);
      return { data: { id: "ses_resumed", directory: "/work" } };
    },
  });
  assert.deepEqual(JSON.parse(JSON.stringify(requests)), [{ path: { id: "ses_resumed" } }]);
  assert.deepEqual(
    resumed.posts.map(({ body }) => [body.session_id, body.source]),
    [["ses_resumed", "session.get"]],
  );

  const unknown = await loadBinding({
    argv: ["--session=ses_requested"],
    sessionGet: async () => ({ data: { id: "ses_other" } }),
  });
  assert.deepEqual(unknown.posts, []);
});

test("resumed_server_worker_binds_the_launch_session_without_parent_argv", async () => {
  const requests: unknown[] = [];
  const binding = await loadBinding({
    argv: ["opencode", "worker.js"],
    resumeSession: "ses_owned",
    sessionGet: async (args) => {
      requests.push(args);
      return { data: { id: "ses_owned", directory: "/work" } };
    },
  });
  assert.deepEqual(JSON.parse(JSON.stringify(requests)), [{ path: { id: "ses_owned" } }]);
  assert.deepEqual(binding.posts.map(({ body }) => [body.session_id, body.source]),
    [["ses_owned", "session.get"]]);
  const mismatch = await loadBinding({
    resumeSession: "ses_owned",
    sessionGet: async () => ({ data: { id: "ses_other" } }),
  });
  assert.deepEqual(mismatch.posts, []);
  const fresh = await loadBinding({
    resumeSession: "",
    sessionGet: async () => { throw new Error("fresh launch must not read a historical session"); },
  });
  assert.deepEqual(fresh.posts, []);
});

test("provider_lifecycle_events_report_the_bound_session", async () => {
  const binding = await loadBinding();
  const error: Event = {
    type: "session.error",
    properties: { sessionID: "ses_failed", error: { name: "ProviderError" } },
  };
  const idle: Event = { type: "session.idle", properties: { sessionID: "ses_failed" } };
  const busy: Event = {
    type: "session.status",
    properties: { sessionID: "ses_failed", status: { type: "busy" } },
  };
  await binding.hooks.event({ event: error });
  await binding.hooks.event({ event: idle });
  await binding.hooks.event({ event: busy });

  assert.deepEqual(
    binding.posts.map(({ url }) => url),
    [
      "http://dev/api/tabs/tab-1/hook/session_start",
      "http://dev/api/tabs/tab-1/hook/provider_event",
      "http://dev/api/tabs/tab-1/hook/provider_event",
      "http://dev/api/tabs/tab-1/hook/provider_event",
    ],
  );
  assert.deepEqual(
    binding.posts.slice(1).map(({ body }) => [body.session_id, body.event]),
    [
      ["ses_failed", error],
      ["ses_failed", idle],
      ["ses_failed", busy],
    ],
  );
});
