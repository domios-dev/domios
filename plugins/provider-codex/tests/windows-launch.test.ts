import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import vm from "node:vm";
import { spawnSync } from "node:child_process";

if (process.platform !== "win32") {
  console.log("SKIP Windows PowerShell instruction serialization on this platform");
} else {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-launch-contract-"));
  try {
    const token = 'opaque-launch-token "quoted"\nsecond-line \\ ${notAnExpression}';
    const taskPath = path.join(root, "instructions.txt");
    const receiptPath = path.join(root, "receipt.json");
    fs.writeFileSync(taskPath, token, "utf8");
    fs.mkdirSync(path.join(root, ".codex"));
    const quote = (value: string) => "'" + value.replace(/'/g, "''") + "'";
    const context = {
      module: { exports: {} as any }, exports: {},
      host: {
        manifestJson: () => "null", platform: () => "windows",
        fs: { writeTempFile: () => taskPath }, shell: { quoteFor: quote },
      },
    };
    vm.runInNewContext(fs.readFileSync("plugins/provider-codex/plugin.js", "utf8"), context);
    const command = context.module.exports.default.buildLaunchCommand({ task: token, silent: true, instanceId: "instance-launch-test" });
    const script = `$ErrorActionPreference = 'Stop'
$env:USERPROFILE = ${quote(root)}
$env:CODETERM_TAB_ID = 'owned-launch-fixture'
function codex {
  $profilePath = Join-Path $env:USERPROFILE '.codex/codeterm-owned-launch-fixture.config.toml'
  $profileText = [IO.File]::ReadAllText($profilePath)
  $lines = $profileText -split "\`r?\`n"
  if ($lines[0] -ne '# codeterm-instance: instance-launch-test') { throw 'instance marker is not the first profile line' }
  $assignment = $profileText.Substring($profileText.IndexOf('developer_instructions = ') + 'developer_instructions = '.Length)
  $value = $assignment | ConvertFrom-Json
  if ($value -isnot [string]) { throw 'instructions were serialized as a provider object' }
  [IO.File]::WriteAllText(${quote(receiptPath)}, ($value | ConvertTo-Json -Compress))
}
${command}`;
    for (const shell of ["powershell.exe", "pwsh.exe"]) {
      fs.rmSync(receiptPath, { force: true });
      const result = spawnSync(shell, ["-NoProfile", "-NonInteractive", "-EncodedCommand", Buffer.from(script, "utf16le").toString("base64")], { encoding: "utf8" });
      if ((result.error as NodeJS.ErrnoException | undefined)?.code === "ENOENT" && shell === "pwsh.exe") {
        console.log("SKIP PowerShell 7 is not installed");
        continue;
      }
      assert.equal(result.status, 0, result.stderr || result.error?.message);
      assert.equal(JSON.parse(fs.readFileSync(receiptPath, "utf8")), token);
      assert.equal(fs.readdirSync(path.join(root, ".codex")).length, 0);
      assert.equal(fs.readFileSync(taskPath, "utf8"), token);
      console.log(`PASS ${shell} exact instruction round trip, retained brief, and owned profile cleanup`);
    }
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}
