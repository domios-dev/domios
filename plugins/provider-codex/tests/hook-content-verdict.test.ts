/**
 * Codex trusts a hook by content hash, so currency is decided by exact
 * generated content, normalizing line endings for Windows file writes.
 *
 * Run: node node_modules/tsx/dist/cli.mjs plugins/provider-codex/tests/hook-content-verdict.test.ts
 *
 * Asserted on the hash and contract MECHANISM. No assertion here quotes a
 * notification, a log line, or a sentence of the script.
 */

import {
  DOMIOS_CODEX_HOOK_SCRIPT_CONTRACT,
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

// Built from the generator, so the fixture matches the installed helper.
const satisfiesContract = (extra = "") =>
  codexWindowsHookScript() + extra;

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

console.log("\n--- only line endings are normalized; content edits are stale ---\n");

{
  const base = satisfiesContract();
  const lineFeed = base.split("\r\n").join("\n");
  const windowsLineEndings = lineFeed.split("\n").join("\r\n");
  eq(isCurrentCodexHookScriptContent(lineFeed), true, "LF line endings preserve freshness");
  eq(isCurrentCodexHookScriptContent(windowsLineEndings), true, "CRLF line endings preserve freshness");
  const edited = satisfiesContract("rem edited helper\n");

  eq(
    codexHookContentHash(base) === codexHookContentHash(edited),
    false,
    "different content hashes differently",
  );
  eq(
    isCurrentCodexHookScriptContent(edited),
    false,
    "an added helper command is stale",
  );
}

for (const token of DOMIOS_CODEX_HOOK_SCRIPT_CONTRACT) {
  const missing = DOMIOS_CODEX_HOOK_SCRIPT_CONTRACT.filter((t) => t !== token).join("\n");
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
  const portFallback = satisfiesContract("$port = if ($env:DOMIOS_API_PORT) { $env:DOMIOS_API_PORT }\n");
  eq(isCurrentCodexHookScriptContent(portFallback), false, "a port-variable fallback is never current");
  const hostFallback = satisfiesContract("$base = \"http://127.0.0.1:$port\"\n");
  eq(isCurrentCodexHookScriptContent(hostFallback), false, "a loopback-host fallback is never current");
}

{
  const generated = codexWindowsHookScript();
  eq(isCurrentCodexHookScriptContent(generated), true, "the generated script satisfies its own contract");
  eq(generated.indexOf("DOMIOS_API_PORT"), -1, "the generated script reads no port variable");
  const oldCleanup = generated + "\r\nrem codeterm-profile-cleanup-v1\r\n"
    + 'del /F /Q "%USERPROFILE%\\.codex\\codeterm-%CODETERM_TAB_ID%.config.toml"';
  eq(isCurrentCodexHookScriptContent(oldCleanup), false,
    "a helper that deletes the active profile is stale and reaches consented repair");
}

{
  const foreign = "# someone else's hook\nexit 0\n";
  eq(isOwnedCodexHookScriptContent(foreign), false, "a hook we do not own is not ours to judge");
  eq(isCurrentCodexHookScriptContent(foreign), false, "a hook we do not own is never rewritten as ours");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
