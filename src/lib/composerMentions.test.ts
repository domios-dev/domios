// Run: npx tsx src/lib/composerMentions.test.ts

import { activeMentionQuery, applyComposerMention, filterComposerMentions, COMPOSER_MENTIONS } from "./composerMentions";

type TestFn = () => void;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const DS_TASKS = COMPOSER_MENTIONS.find((mention) => mention.token === "@ds-tasks")!;

test("only a trailing @word at a word boundary is a mention query", () => {
  assert(activeMentionQuery("hi @ds")?.partial === "@ds", "trailing partial");
  assert(activeMentionQuery("@")?.start === 0, "lone @ at start");
  assert(activeMentionQuery("mail@ds") === null, "glued to a word");
  assert(activeMentionQuery("@ds then") === null, "not at the end");
});

test("suggestions match by prefix and disappear once the token is complete", () => {
  assert(filterComposerMentions(activeMentionQuery("@d")).includes(DS_TASKS), "prefix match");
  assert(filterComposerMentions(activeMentionQuery("@DS-T")).includes(DS_TASKS), "case-insensitive");
  assert(filterComposerMentions(activeMentionQuery("@ds-tasks")).length === 0, "complete token");
  assert(filterComposerMentions(activeMentionQuery("@zz")).length === 0, "no match");
  assert(filterComposerMentions(null).length === 0, "no query");
});

test("picking replaces the partial with the token and a trailing space", () => {
  const draft = "show me @ds-t";
  const query = activeMentionQuery(draft)!;
  assert(applyComposerMention(draft, query, DS_TASKS) === "show me @ds-tasks ", "inserted token");
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (error) {
    failed++;
    console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
  }
}
if (failed > 0) process.exit(1);
console.log(`\n✓ all ${tests.length} composerMentions tests green`);
