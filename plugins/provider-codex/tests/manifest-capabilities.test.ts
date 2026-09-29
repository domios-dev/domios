import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

let passed = 0;
let failed = 0;

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) { passed++; console.log(`  ✓ ${name}`); }
  else { failed++; console.error(`  ✗ ${name}: expected ${e}, got ${a}`); }
}

const manifest = JSON.parse(fs.readFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.json"), "utf8"));
eq(manifest.spawn.pasteLimitBytes, 4096, "paste limit capability is declared in bytes");
eq(manifest.commands.additionalWritableDirFlag, "--add-dir", "sandbox metadata roots use the declared CLI flag");
eq(manifest.spawn.sandboxedLinuxLaunch.networkAccessArgs,
  ["-c", "sandbox_workspace_write.network_access=true"],
  "Linux sandbox network access is declared through the provider capability");
eq(manifest.spawn.sandboxedLinuxLaunch.networkAccessConfigPath, "~/.codex/config.toml",
  "existing Codex config is read to avoid repeating a network grant");

console.log(`
${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
