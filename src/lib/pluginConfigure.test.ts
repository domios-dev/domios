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

// configureWithAI must bring the spawned Configure pane forward (Bug G fix):
// it dispatches `codeterm:focus-pane` with the spawned pane id (App focuses it).
async function asyncTests() {
  console.log("\nconfigureWithAI:");
  const focusEvents: Array<{ type: string; detail?: { paneId?: string } }> = [];
  const g = globalThis as unknown as Record<string, unknown>;
  g.window = {
    dispatchEvent: (e: { type: string; detail?: { paneId?: string } }) => (focusEvents.push(e), true),
  };
  g.CustomEvent = class<T> {
    type: string;
    detail: T;
    constructor(type: string, init?: { detail: T }) {
      this.type = type;
      this.detail = init?.detail as T;
    }
  };
  g.fetch = async () => ({ json: async () => ({ id: "pane-xyz" }) });
  await configureWithAI("lmstudio", "LM Studio", "help", { host: "127.0.0.1", port: 7686 });
  const focus = focusEvents.find((e) => e.type === "codeterm:focus-pane");
  assert("dispatches codeterm:focus-pane with the spawned pane id", focus?.detail?.paneId === "pane-xyz");
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
