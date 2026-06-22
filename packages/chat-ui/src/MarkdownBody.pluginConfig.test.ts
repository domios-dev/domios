// Run: npx tsx packages/chat-ui/src/MarkdownBody.pluginConfig.test.ts
import { parsePluginConfigMessage } from "./MarkdownBody";

let passed = 0,
  failed = 0;
function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}: expected ${e}, got ${a}`);
  }
}

eq(
  parsePluginConfigMessage(
    "-=-codeterm:plugin_config plugin=lmstudio name=LM%20Studio-=-do x",
  ),
  { pluginId: "lmstudio", name: "LM Studio", body: "do x" },
  "parses plugin_config marker into pluginId/name/body",
);

eq(
  parsePluginConfigMessage("just a plain message"),
  null,
  "returns null on plain text",
);

eq(
  parsePluginConfigMessage("-=-codeterm:voice file=/u/v.webm-=-hi"),
  null,
  "returns null on a non-plugin_config marker",
);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
