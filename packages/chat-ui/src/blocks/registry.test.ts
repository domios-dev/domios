// Run: npx tsx packages/chat-ui/src/blocks/registry.test.ts

import { isolateBlockDirectives, parseChatBlock, parseChatBlocks, type ChatBlockDefinition } from "./registry";
import { parsePluginId } from "./pluginBlockModel";

const tests: Array<[string, () => void]> = [];
function test(name: string, run: () => void) {
  tests.push([name, run]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("directives_under_a_lead_line_get_their_own_paragraph_outside_fences", () => {
  const isolated = isolateBlockDirectives("Phone:\n@ds-guide phone\nPlugins:\n@ds-plugin git\n```\n@ds-plugin git\n```");
  assert(isolated === "Phone:\n\n@ds-guide phone\n\nPlugins:\n\n@ds-plugin git\n\n```\n@ds-plugin git\n```", `unexpected isolation: ${JSON.stringify(isolated)}`);
  assert(isolateBlockDirectives("plain text") === "plain text", "text without directives changed");
});

test("plugin_directive_parses_a_standalone_line_with_a_valid_id", () => {
  const block = parseChatBlock("@ds-plugin telegram-client");
  assert(block?.name === "plugin" && block.source === "@ds-plugin telegram-client", "plugin block lost");
  assert(parseChatBlock("\t@ds-plugin teams-client \r")?.source === "@ds-plugin teams-client", "indent or CR broke parsing");
});

test("plugin_ids_are_validated", () => {
  for (const id of ["telegram-client", "chat-preprocessor", "a", "my_plugin.v2"]) {
    assert(parsePluginId(id) === id, `rejected valid id ${id}`);
  }
  for (const id of ["../etc", "Telegram", "-lead", "a b", "x..y", "https://example.com/p", "", "a".repeat(65)]) {
    assert(parsePluginId(id) === null, `accepted invalid id ${id}`);
  }
  assert(parseChatBlock("@ds-plugin ../secrets") === null, "invalid id became a block");
});

test("non_standalone_or_unknown_directives_stay_text", () => {
  for (const source of ["see @ds-plugin telegram-client", "@ds-plugin telegram-client now", "@ds-plugin", "`@ds-plugin x`", "@ds-unknown thing", "@ds-plugin a\nb"]) {
    assert(parseChatBlock(source) === null, `accepted ${JSON.stringify(source)}`);
  }
});

test("guide_directive_is_served_by_the_same_registry", () => {
  assert(parseChatBlock("@ds-guide phone")?.name === "guide", "guide not registered");
  assert(parseChatBlock("@ds-guide phone extra") === null, "guide accepted trailing text");
});

test("consecutive_block_lines_render_as_stacked_blocks_and_any_text_line_cancels_them", () => {
  const stacked = parseChatBlocks("@ds-guide messenger-plugins\n@ds-plugin telegram-client\n@ds-plugin teams-client");
  assert(stacked?.map((block) => block.name).join(",") === "guide,plugin,plugin", "stacked blocks lost");
  assert(parseChatBlocks("@ds-plugin telegram-client\nInstall it first.") === null, "mixed paragraph became blocks");
});

test("connect_and_setting_directives_are_registered_and_validate_their_argument", () => {
  assert(parseChatBlock("@ds-connect telegram")?.source === "@ds-connect telegram", "connect block lost");
  assert(parseChatBlock("@ds-setting plugins.chat-preprocessor.enabled")?.name === "setting", "setting block lost");
  for (const source of ["@ds-connect Telegram", "@ds-connect tele gram", "@ds-setting enabled", "@ds-setting a..b", "@ds-setting plugins.x.y now"]) {
    assert(parseChatBlock(source) === null, `accepted ${JSON.stringify(source)}`);
  }
  const stacked = parseChatBlocks("@ds-guide phone\n@ds-connect telegram\n@ds-setting plugins.x.enabled");
  assert(stacked?.map((block) => block.name).join(",") === "guide,connect,setting", "stacked new blocks lost");
});

test("a_new_block_plugs_in_with_a_parser_and_renderer", () => {
  const rendered: string[] = [];
  const choice: ChatBlockDefinition<string[]> = {
    name: "choice",
    parse: (argument) => (argument.includes("|") ? argument.split("|") : null),
    render: (options) => {
      rendered.push(options.join(","));
      return null;
    },
  };
  const block = parseChatBlock("@ds-choice yes|no", [choice as ChatBlockDefinition<unknown>]);
  assert(block !== null && rendered.length === 0, "parsing must not render");
  block.render();
  assert(rendered[0] === "yes,no", "renderer did not receive parsed props");
});

let failed = 0;
for (const [name, run] of tests) {
  try {
    run();
    console.log(`  ✓ ${name}`);
  } catch (error) {
    failed++;
    console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
  }
}
console.log(`chat-ui/blocks/registry tests: ${failed === 0 ? "all passed" : `${failed} failed`}`);
process.exit(failed === 0 ? 0 : 1);
