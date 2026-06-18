import { parseMarker } from "@codeterm/chat-core";
import { buildConfigureTask, CONFIG_PREAMBLE } from "./pluginConfigure";

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

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
