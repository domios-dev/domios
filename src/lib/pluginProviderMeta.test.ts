// Run: npx tsx src/lib/pluginProviderMeta.test.ts
//
// The plugin-snapshot (`plugin_manager_snapshot`) carries per-plugin identity in
// snake_case. pluginProviderMetaFromSnapshot maps it into the camelCase shape the
// resolver consumes, and setPluginProviderMeta/getPluginProviderMeta cache it.

import {
  getPluginProviderMeta,
  pluginProviderMetaFromSnapshot,
  setPluginProviderMeta,
} from "./pluginProviderMeta";

const tests: Array<[string, () => void]> = [];
function test(name: string, fn: () => void) {
  tests.push([name, fn]);
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

test("maps snake_case snapshot entries into camelCase meta", () => {
  const meta = pluginProviderMetaFromSnapshot({
    plugins: [
      {
        id: "lmstudio",
        display_name: "LM Studio",
        icon_svg: "<svg/>",
        icon_color: "#ff6600",
        icon_hint: "LM",
        available_models: [{ id: "qwen-2.5", display_name: "Qwen 2.5" }],
        default_model: "qwen-2.5",
      },
    ],
  });
  assert(meta.length === 1, `expected 1 entry, got ${meta.length}`);
  const m = meta[0];
  assert(m.id === "lmstudio", "id");
  assert(m.displayName === "LM Studio", "displayName");
  assert(m.iconSvg === "<svg/>", "iconSvg");
  assert(m.iconColor === "#ff6600", "iconColor");
  assert(m.iconHint === "LM", "iconHint");
  assert(m.availableModels?.[0]?.id === "qwen-2.5", "availableModels");
  assert(m.defaultModel === "qwen-2.5", "defaultModel");
});

test("falls back displayName to id when absent", () => {
  const meta = pluginProviderMetaFromSnapshot({ plugins: [{ id: "bare" }] });
  assert(meta[0].displayName === "bare", "displayName falls back to id");
});

test("skips entries without an id", () => {
  const meta = pluginProviderMetaFromSnapshot({
    plugins: [{ display_name: "no id" }, { id: "ok" }],
  });
  assert(meta.length === 1 && meta[0].id === "ok", "drops id-less entries");
});

test("tolerates an empty / null snapshot", () => {
  assert(pluginProviderMetaFromSnapshot(null).length === 0, "null snapshot -> []");
  assert(pluginProviderMetaFromSnapshot({}).length === 0, "empty snapshot -> []");
});

test("set/get round-trips by id", () => {
  setPluginProviderMeta([{ id: "lmstudio", displayName: "LM Studio" }]);
  assert(getPluginProviderMeta("lmstudio")?.displayName === "LM Studio", "cached by id");
  assert(getPluginProviderMeta("nope") === undefined, "unknown id -> undefined");
  assert(getPluginProviderMeta(null) === undefined, "null id -> undefined");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`✓ ${name}`);
  } catch (err) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(err);
  }
}
console.log(`pluginProviderMeta: ${tests.length - failed}/${tests.length} passed`);
if (failed > 0) process.exit(1);
