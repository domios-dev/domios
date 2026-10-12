import { configureDaemonTransport } from "./daemonBootstrap";
import { parseMarker } from "@codeterm/chat-core";
import { AI_HELPER_CONTEXT, AI_HELPER_SPAWN_PATH } from "./aiHelperSpawn";
import { buildConfigureTask, configureWithAI, PLUGIN_CONFIG_RULES } from "./pluginConfigure";

let passed = 0;
let failed = 0;

function assert(label: string, cond: boolean) {
  if (cond) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}`);
    failed++;
  }
}

console.log("buildConfigureTask:");

const helpToken = "plugin-config-help-token-5b02";
const task = buildConfigureTask("lmstudio", "LM Studio", helpToken);
const parsed = parseMarker(task);
assert("parses to a marker", parsed !== null);
assert("kind is plugin_config", parsed?.kind === "plugin_config");
assert("plugin attr round-trips", parsed?.attrs.plugin === "lmstudio");
assert("name attr round-trips", parsed?.attrs.name === "LM Studio");
const payload = parsed?.payload ?? "";
assert("payload carries the plugin configHelp", payload.includes(helpToken));
assert("payload carries every helper context fragment", AI_HELPER_CONTEXT.length > 0 && AI_HELPER_CONTEXT.every((fragment) => payload.includes(fragment)));
assert("payload carries every plugin config rule", PLUGIN_CONFIG_RULES.length > 0 && PLUGIN_CONFIG_RULES.every((rule) => payload.includes(rule)));
assert("core context precedes the plugin-authored help", PLUGIN_CONFIG_RULES.every((rule) => payload.indexOf(rule) < payload.indexOf(helpToken)));

async function asyncTests() {
  console.log("\nconfigureWithAI:");
  const focusEvents: Array<{ type: string; detail?: { tabId?: string; workspaceId?: string } }> = [];
  const g = globalThis as unknown as Record<string, unknown>;
  g.window = {
    location: { origin: "http://127.0.0.1:7686" },
    dispatchEvent: (e: { type: string; detail?: { tabId?: string; workspaceId?: string } }) => (focusEvents.push(e), true),
  };
  g.CustomEvent = class<T> {
    type: string;
    detail: T;
    constructor(type: string, init?: { detail: T }) {
      this.type = type;
      this.detail = init?.detail as T;
    }
  };
  configureDaemonTransport({ origin: "http://127.0.0.1:7686", token: "fixture-token" });

  const opaqueToken = process.env.PROVIDER_SETUP_TASK_TOKEN ?? "configure-token-7f91b";
  let requestUrl = "";
  let requestBody: Record<string, unknown> = {};
  g.fetch = async (url: string, init: { body?: string }) => {
    requestUrl = url;
    requestBody = JSON.parse(init.body ?? "{}");
    return { ok: true, status: 200, json: async () => ({ id: "tab-xyz", workspace_id: "workspace-general", delivery_state: "launched" }) };
  };
  await configureWithAI("lmstudio", "LM Studio", opaqueToken, { host: "127.0.0.1", port: 7686 });
  const focus = focusEvents.find((e) => e.type === "codeterm:focus-tab");
  assert("spawns through the shared daemon helper route", requestUrl.endsWith(AI_HELPER_SPAWN_PATH));
  assert(
    "leaves provider and placement to the daemon",
    !("agent" in requestBody) && !("workspace_id" in requestBody) && !("cwd" in requestBody),
  );
  assert("uses one-word tab title and descriptive plugin context", requestBody.title === "Configure" && !/\s/.test(String(requestBody.title)) && requestBody.description === "Configure LM Studio");
  assert("delivers the supplied opaque token in the spawn task", String(requestBody.task).includes(opaqueToken));
  assert("dispatches focus with the spawned Tab and General workspace ids", focus?.detail?.tabId === "tab-xyz" && focus?.detail?.workspaceId === "workspace-general");

  const exactFailure = "fixture launch refused 2c418";
  g.fetch = async () => ({
    ok: false,
    status: 503,
    json: async () => ({ error: { message: exactFailure } }),
  });
  let surfaced = "";
  try {
    await configureWithAI("lmstudio", "LM Studio", opaqueToken, { host: "127.0.0.1", port: 7686 });
  } catch (error) {
    surfaced = error instanceof Error ? error.message : String(error);
  }
  assert("surfaces the exact launch or delivery failure", surfaced === exactFailure);
}

asyncTests()
  .catch((e) => {
    console.error(e);
    failed++;
  })
  .finally(() => {
    console.log(`\n${passed} passed, ${failed} failed`);
    if (failed > 0) process.exit(1);
  });
