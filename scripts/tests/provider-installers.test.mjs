import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

for (const provider of ["claude", "codex", "opencode"]) {
  test(`${provider} installer invokes its noninteractive fake without another prompt`, { skip: process.platform !== "win32" }, () => {
    const manifest = JSON.parse(readFileSync(new URL(`../../plugins/provider-${provider}/plugin.json`, import.meta.url), "utf8"));
    const directory = mkdtempSync(path.join(tmpdir(), "domios-installer-"));
    const body = provider === "codex"
      ? 'if ($env:CODEX_NON_INTERACTIVE -ne "1") { throw "Installer would prompt" }; Write-Output "INSTALL_FINISHED"'
      : 'Write-Output "INSTALL_FINISHED"';
    const escaped = body.replaceAll("'", "''");
    const fake = `
      $ErrorActionPreference = 'Stop'
      function Read-Host { throw 'Unexpected interactive prompt' }
      function Invoke-WebRequest { param($Uri, $OutFile, [switch]$UseBasicParsing); Set-Content -LiteralPath $OutFile -Value '${escaped}' }
      function npm { if (($args -join ' ') -ne 'install -g opencode-ai') { throw 'Unexpected npm invocation' }; Write-Output 'INSTALL_FINISHED' }
    `;
    try {
      const result = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", fake + manifest.installer.windows], {
        encoding: "utf8", timeout: 10_000, env: { ...process.env, TEMP: directory, CODEX_NON_INTERACTIVE: "" },
      });
      assert.equal(result.error, undefined);
      assert.equal(result.status, 0, result.stderr);
      assert.match(result.stdout, /INSTALL_FINISHED/);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
}
