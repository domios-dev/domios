/**
 * Codex trusts a hook by content hash, so currency has to be decided on
 * content. It used to be decided by comparing the file to the running build's
 * own template, which made every cross-instance start declare the other's hook
 * stale — a rewrite, a broken trusted_hash, and a dead session binding.
 *
 * Run: node node_modules/tsx/dist/cli.mjs plugins/provider-codex/tests/hook-content-verdict.test.ts
 *
 * Asserted on the hash and contract MECHANISM. No assertion here quotes a
 * notification, a log line, or a sentence of the script.
 */

import {
  CODEX_HOOK_SCRIPT_CONTRACT,
  codexHookContentHash,
  isCurrentCodexHookScriptContent,
  isOwnedCodexHookScriptContent,
} from "../src/hooks";

let passed = 0;
let failed = 0;

function eq(actual: unknown, expected: unknown, name: string) {
  if (actual === expected) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}: expected ${String(expected)}, got ${String(actual)}`); }
}

// Built from the contract itself, so this fixture cannot drift from the rule.
const satisfiesContract = (extra = "") =>
  CODEX_HOOK_SCRIPT_CONTRACT.join("\n") + "\n" + extra;

console.log("\n--- byte-identical content is current for both writers ---\n");

{
  // Two instances (a dev build and an installed prod build) writing the same
  // bytes. Nothing here names which one wrote it.
  const writtenByA = satisfiesContract();
  const writtenByB = satisfiesContract();

  eq(writtenByA === writtenByB, true, "the two instances wrote byte-identical content");
  eq(
    codexHookContentHash(writtenByA),
    codexHookContentHash(writtenByB),
    "byte-identical content hashes identically, so the trust record cannot disagree",
  );
  eq(isCurrentCodexHookScriptContent(writtenByA), true, "current for the first instance");
  eq(isCurrentCodexHookScriptContent(writtenByB), true, "current for the second instance");
  eq(
    isCurrentCodexHookScriptContent(writtenByA),
    isCurrentCodexHookScriptContent(writtenByB),
    "one verdict for one content, whoever wrote it",
  );
}

console.log("\n--- only differing content changes the verdict or the hash ---\n");

{
  const base = satisfiesContract();
  const reformatted = satisfiesContract("# a later build reformatted the script\n");

  eq(
    codexHookContentHash(base) === codexHookContentHash(reformatted),
    false,
    "different content hashes differently",
  );
  eq(
    isCurrentCodexHookScriptContent(reformatted),
    true,
    "a reformat that still satisfies the contract is not stale",
  );
}

for (const token of CODEX_HOOK_SCRIPT_CONTRACT) {
  const missing = CODEX_HOOK_SCRIPT_CONTRACT.filter((t) => t !== token).join("\n");
  eq(
    isCurrentCodexHookScriptContent(missing),
    false,
    `content missing a contract token is stale (${token.slice(0, 12)})`,
  );
}

{
  // A baked-in port is the specific bug the standing rule forbids: it silently
  // sends one instance's agents to the other.
  const withPort = satisfiesContract("http://127.0.0.1:7685/api/tabs/x\n");
  eq(isCurrentCodexHookScriptContent(withPort), false, "instance-specific content is never current");
}

{
  const foreign = "# someone else's hook\nexit 0\n";
  eq(isOwnedCodexHookScriptContent(foreign), false, "a hook we do not own is not ours to judge");
  eq(isCurrentCodexHookScriptContent(foreign), false, "a hook we do not own is never rewritten as ours");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
