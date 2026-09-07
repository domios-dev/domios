import { parseMarker } from "@codeterm/chat-core";
import { buildConfigureTask, CONFIG_PREAMBLE, configureWithAI } from "./pluginConfigure";

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

const task = buildConfigureTask("lmstudio", "LM Studio", "set the model");

assert("includes the core preamble", task.includes(CONFIG_PREAMBLE));
assert(
  "wraps help in the safety envelope",
  task.includes("<PLUGIN CONFIG — ASK THE USER, DO NOT ACT AUTOMATICALLY>") &&
    task.includes("set the model") &&
    task.includes("</PLUGIN CONFIG>"),
);

const parsed = parseMarker(task);
assert("parses to a marker", parsed !== null);
assert("kind is plugin_config", parsed?.kind === "plugin_config");
assert("plugin attr round-trips", parsed?.attrs.plugin === "lmstudio");
assert("name attr round-trips", parsed?.attrs.name === "LM Studio");
assert("payload carries the wrapped help", parsed?.payload.includes("set the model") ?? false);

async function asyncTests() {
  console.log("\nconfigureWithAI:");
  const focusEvents: Array<{ type: string; detail?: { tabId?: string } }> = [];
  const g = globalThis as unknown as Record<string, unknown>;
  g.window = {
    dispatchEvent: (e: { type: string; detail?: { tabId?: string } }) => (focusEvents.push(e), true),
  };
  g.CustomEvent = class<T> {
    type: string;
    detail: T;
    constructor(type: string, init?: { detail: T }) {
      this.type = type;
      this.detail = init?.detail as T;
    }
  };
  const opaqueToken = process.env.PROVIDER_SETUP_TASK_TOKEN ?? "configure-token-7f91b";
  let requestBody: Record<string, unknown> = {};
  g.fetch = async (_url: string, init: { body?: string }) => {
    requestBody = JSON.parse(init.body ?? "{}");
    return { ok: true, status: 200, json: async () => ({ id: "tab-xyz", delivery_state: "confirmed" }) };
  };
  await configureWithAI("lmstudio", "LM Studio", opaqueToken, { host: "127.0.0.1", port: 7686 });
  const focus = focusEvents.find((e) => e.type === "codeterm:focus-tab");
  assert("leaves provider resolution to the configured General-Agent contract", !("agent" in requestBody));
  assert("requests observable delivery acknowledgement", requestBody.require_delivery_ack === true);
  assert("delivers the supplied opaque token in the spawn task", String(requestBody.task).includes(opaqueToken));
  assert("dispatches codeterm:focus-tab with the spawned Tab id", focus?.detail?.tabId === "tab-xyz");

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
