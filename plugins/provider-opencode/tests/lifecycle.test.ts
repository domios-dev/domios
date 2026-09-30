// Run: npx tsx plugins/provider-opencode/tests/lifecycle.test.ts

import assert from "node:assert/strict";
import { test } from "node:test";
import vm from "node:vm";
import { opencodeBindingPluginSource } from "../src/helpers";

type Event = { type: string; properties?: Record<string, unknown> };
type Hooks = { event: (input: { event: Event }) => Promise<void> };

async function loadBinding(input: {
  argv?: string[];
  sessionGet?: (args: unknown) => Promise<unknown>;
} = {}) {
  const posts: Array<{ url: string; body: Record<string, unknown> }> = [];
  const source = opencodeBindingPluginSource().replace(
    "export default async function CodeTermSessionBindingPlugin",
    "async function CodeTermSessionBindingPlugin",
  );
  const context = {
    process: {
      argv: input.argv || [],
      env: {
        CODETERM_TAB_ID: "tab-1",
        CODETERM_API_TOKEN: "token",
        CODETERM_SESSION_BINDING_NONCE: "nonce",
        CODETERM_API_URL: "http://dev",
      },
      cwd: () => "/work",
    },
    fetch: async (url: string, options: { body: string }) => {
      posts.push({ url, body: JSON.parse(options.body) as Record<string, unknown> });
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
  return { hooks, posts };
}

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
