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
    const releasePath = path.join(root, "release-uri.txt");
    const releaseLogPath = path.join(root, "release-log.txt");
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
$env:CODETERM_API_URL = 'http://127.0.0.1:7686'
$env:CODETERM_API_TOKEN = 'fixture-token'
$script:ReleaseResult = $true
function Invoke-RestMethod {
  [CmdletBinding()]
  param($Method, $Uri, $Headers, $ContentType, $Body)
  [IO.File]::WriteAllText((Join-Path $env:USERPROFILE 'release-uri.txt'), $Uri)
  $profileDir = Join-Path $env:USERPROFILE '.codex'
  $profilePaths = @(Get-ChildItem -LiteralPath $profileDir -Filter 'codeterm-owned-launch-fixture*.config.toml' -File | ForEach-Object FullName)
  foreach ($profilePath in $profilePaths) {
    $profileText = [IO.File]::ReadAllText($profilePath)
    Add-Content -LiteralPath ${quote(releaseLogPath)} -Value $profileText
    if ($script:ReleaseResult -and (($profileText -split "`r?`n")[0] -eq '# codeterm-instance: instance-launch-test')) {
      Remove-Item -LiteralPath $profilePath -Force
    }
  }
  return @{ released = $script:ReleaseResult }
}
function codex {
  $profilePath = Join-Path $env:USERPROFILE ('.codex/' + $ctProfileName + '.config.toml')
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
      for (const name of fs.readdirSync(path.join(root, ".codex"))) {
        fs.rmSync(path.join(root, ".codex", name), { recursive: true, force: true });
      }
      fs.rmSync(receiptPath, { force: true });
      fs.rmSync(releaseLogPath, { force: true });
      fs.writeFileSync(
        path.join(root, ".codex", "codeterm-owned-launch-fixture.config.toml"),
        "# codeterm-instance: instance-launch-test\n[projects.'C:\\Previous']\ntrust_level = 'trusted'\n",
      );
      const result = spawnSync(shell, ["-NoProfile", "-NonInteractive", "-EncodedCommand", Buffer.from(script, "utf16le").toString("base64")], { encoding: "utf8" });
      if ((result.error as NodeJS.ErrnoException | undefined)?.code === "ENOENT" && shell === "pwsh.exe") {
        console.log("SKIP PowerShell 7 is not installed");
        continue;
      }
      assert.equal(result.status, 0, result.stderr || result.error?.message);
      assert.equal(JSON.parse(fs.readFileSync(receiptPath, "utf8")), token);
      assert.equal(fs.readdirSync(path.join(root, ".codex")).length, 0);
      assert.equal(
        fs.readFileSync(releasePath, "utf8"),
        "http://127.0.0.1:7686/api/tabs/owned-launch-fixture/launch-artifacts/release",
      );
      const releasedProfiles = fs.readFileSync(releaseLogPath, "utf8");
      assert.match(releasedProfiles, /\[projects\.'C:\\Previous'\]/,
        "an uncarried profile is released before the next launch can overwrite it");
      assert.equal(releasedProfiles.split("# codeterm-instance:").length - 1, 2,
        "prior and current profiles both pass through release");
      assert.equal(fs.readFileSync(taskPath, "utf8"), token);
      console.log(`PASS ${shell} exact instruction round trip, retained brief, and owned profile cleanup`);

      const unownedProfile = "developer_instructions = 'foreign profile data'\n";
      const profilePath = path.join(root, ".codex", "codeterm-owned-launch-fixture.config.toml");
      fs.writeFileSync(profilePath, unownedProfile, "utf8");
      fs.rmSync(receiptPath, { force: true });
      const refusedScript = script.replace("$script:ReleaseResult = $true", "$script:ReleaseResult = $false");
      const refused = spawnSync(shell, ["-NoProfile", "-NonInteractive", "-EncodedCommand", Buffer.from(refusedScript, "utf16le").toString("base64")], { encoding: "utf8" });
      assert.equal(refused.status, 0, refused.stderr || refused.error?.message);
      assert.equal(JSON.parse(fs.readFileSync(receiptPath, "utf8")), token,
        "Codex still receives the task when a leftover unowned profile cannot be released");
      const retryName = fs.readdirSync(path.join(root, ".codex")).find((name) => name.includes("-retry-"));
      assert.ok(retryName, "an unowned profile is left in place while the launch uses a retry profile");
      assert.equal(fs.readFileSync(profilePath, "utf8"), unownedProfile,
        "the unowned profile is never moved or overwritten");
      assert.match(fs.readFileSync(path.join(root, ".codex", retryName!), "utf8"),
        /^# codeterm-instance: instance-launch-test/);

      for (const name of fs.readdirSync(path.join(root, ".codex"))) {
        fs.rmSync(path.join(root, ".codex", name), { recursive: true, force: true });
      }
      fs.writeFileSync(profilePath,
        "# codeterm-instance: instance-launch-test\n[projects.'C:\\Previous']\ntrust_level = 'trusted'\n", "utf8");
      fs.rmSync(receiptPath, { force: true });
      fs.rmSync(releaseLogPath, { force: true });
      for (let launch = 0; launch < 5; launch++) {
        const refusedLaunch = spawnSync(shell, ["-NoProfile", "-NonInteractive", "-EncodedCommand", Buffer.from(refusedScript, "utf16le").toString("base64")], { encoding: "utf8" });
        assert.equal(refusedLaunch.status, 0, refusedLaunch.stderr || refusedLaunch.error?.message);
      }
      const preservedNames = fs.readdirSync(path.join(root, ".codex")).filter((name) => name.includes("-preserved"));
      assert.equal(preservedNames.length, 1, "repeated refused carry-over keeps at most one preserved profile per tab");
      assert.ok(fs.readdirSync(path.join(root, ".codex")).some((name) => name.includes("-retry-")),
        "a launch continues on a retry profile while the one preserved profile is retained");
    }
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}
