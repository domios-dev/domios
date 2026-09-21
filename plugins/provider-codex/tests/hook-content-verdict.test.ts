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
  CODETERM_CODEX_PROFILE_CLEANUP_MARKER,
  codexWindowsHookScript,
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
  // A script that falls back to an instance when its environment is missing
  // is stale, so startup reconciliation replaces it with the fail-closed one.
  const portFallback = satisfiesContract("$port = if ($env:CODETERM_API_PORT) { $env:CODETERM_API_PORT }\n");
  eq(isCurrentCodexHookScriptContent(portFallback), false, "a port-variable fallback is never current");
  const hostFallback = satisfiesContract("$base = \"http://127.0.0.1:$port\"\n");
  eq(isCurrentCodexHookScriptContent(hostFallback), false, "a loopback-host fallback is never current");
}

{
  const generated = codexWindowsHookScript();
  eq(isCurrentCodexHookScriptContent(generated), true, "the generated script satisfies its own contract");
  eq(generated.indexOf("CODETERM_API_PORT"), -1, "the generated script reads no port variable");
  const cleanup = generated.indexOf(":cleanup_profile");
  const apiGuard = generated.indexOf("if not defined CODETERM_API_URL");
  eq(cleanup > 0, true, "the script has an owned profile cleanup path");
  eq(cleanup < apiGuard, true, "profile cleanup precedes optional network credentials");
  eq(generated.includes('"%hookEvent%"=="stop" goto cleanup_profile'), true,
    "Stop can clean a profile when SessionStart did not");
  const staleCleanup = generated.replace(CODETERM_CODEX_PROFILE_CLEANUP_MARKER, "codeterm-profile-cleanup-v1");
  eq(isCurrentCodexHookScriptContent(staleCleanup), false, "the previous cleanup contract is stale");
}

{
  const foreign = "# someone else's hook\nexit 0\n";
  eq(isOwnedCodexHookScriptContent(foreign), false, "a hook we do not own is not ours to judge");
  eq(isCurrentCodexHookScriptContent(foreign), false, "a hook we do not own is never rewritten as ours");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
