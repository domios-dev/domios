import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import vm from "node:vm";
import { fileURLToPath } from "node:url";

if (process.platform !== "win32") {
  console.log("SKIP Windows commandWindows shells are unavailable on this platform");
} else {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm hook command "));
  let writes = 0;
  try {
    const hooksPath = path.join(root, ".codex", "hooks.json");
    const pluginPath = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "plugin.js");
    const context = {
      host: {
        manifestJson: () => "null",
        homeDir: () => root,
        platform: () => "windows",
        unixNowMs: () => 0,
        path: { equal: (left: string, right: string) => left === right },
        envGet: () => null,
        shell: { quoteFor: (value: unknown) => String(value), quote: (value: unknown) => String(value) },
        log: () => {},
        fs: {
          fileExists: (file: string) => fs.existsSync(file),
          readFile: (file: string) => (
            fs.existsSync(file) ? fs.readFileSync(file, "utf8") : null
          ),
          readFileHead: () => null,
          readFileTail: () => null,
          readJson: (file: string) => (
            fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, "utf8")) : null
          ),
          writeFile: (file: string, content: string) => {
            writes += 1;
            fs.mkdirSync(path.dirname(file), { recursive: true });
            fs.writeFileSync(file, content, "utf8");
            return true;
          },
          writeJson: (file: string, value: unknown) => {
            writes += 1;
            fs.mkdirSync(path.dirname(file), { recursive: true });
            fs.writeFileSync(file, JSON.stringify(value), "utf8");
            return true;
          },
          removeFile: (file: string) => {
            fs.rmSync(file, { force: true });
            return true;
          },
        },
      },
      console,
      module: {
        exports: {} as {
          default?: {
            installIntegration: (port: number, options: { consent: boolean }) => { ok: boolean };
            checkIntegration: () => Record<string, unknown>;
            ensureIntegrationOnStartup: (port: number) => { actionable?: boolean };
          };
        },
      },
      exports: {},
    };
    new vm.Script(fs.readFileSync(pluginPath, "utf8"), { filename: pluginPath }).runInNewContext(context);
    const plugin = context.module.exports.default;
    assert(plugin, "Codex provider bundle loads");
    assert(plugin.installIntegration(7686, { consent: true }).ok, "Codex hook integration installs");

    const installed = JSON.parse(fs.readFileSync(hooksPath, "utf8")) as {
      hooks: {
        SessionStart: Array<{ hooks: Array<{ commandWindows: string }> }>;
        Stop: Array<{ hooks: Array<{ commandWindows: string }> }>;
      };
    };
    const command = installed.hooks.SessionStart[0]?.hooks[0]?.commandWindows ?? "";
    assert(command.startsWith('cmd.exe /d /s /c call "'), "registered command wraps a quoted helper path", command);
    const currentStatus = plugin.checkIntegration();
    assert(currentStatus.hook_contract_current === true, "the installed cross-shell command is recognized as current");
    assert(
      plugin.ensureIntegrationOnStartup(7686).actionable !== true,
      "startup leaves the current command in place");

    for (const entries of [installed.hooks.SessionStart, installed.hooks.Stop]) {
      for (const entry of entries) {
        const hook = entry.hooks[0];
        if (hook) hook.commandWindows = hook.commandWindows.replace(/^cmd\.exe \/d \/s \/c call /, "");
      }
    }
    fs.writeFileSync(hooksPath, JSON.stringify(installed), "utf8");
    const staleBytes = fs.readFileSync(hooksPath, "utf8");
    const writesBeforeStaleCheck = writes;
    const staleStatus = plugin.checkIntegration();
    assert(staleStatus.hook_contract_current === false, "the old quoted-path command is recognized as stale");
    assert(staleStatus.hook_content_hash !== currentStatus.hook_content_hash,
      "changing the trusted Windows command changes its content hash");
    assert(
      plugin.ensureIntegrationOnStartup(7686).actionable === true,
      "startup reports the stale command without rewriting it or bypassing consent");
    assert.equal(writes, writesBeforeStaleCheck, "stale classification performs no writes without consent");
    assert.equal(fs.readFileSync(hooksPath, "utf8"), staleBytes, "stale hook bytes remain unchanged without consent");
    for (const entries of [installed.hooks.SessionStart, installed.hooks.Stop]) {
      for (const entry of entries) {
        const hook = entry.hooks[0];
        if (hook) hook.commandWindows = command.replace("session_start", "stop");
      }
    }
    installed.hooks.SessionStart[0]!.hooks[0]!.commandWindows = command;
    fs.writeFileSync(hooksPath, JSON.stringify(installed), "utf8");

    const helperPath = path.join(root, ".codex", "codeterm-session-hook.cmd");
    fs.writeFileSync(
      helperPath,
      [
        "@echo off",
        "if not defined CODETERM_TEST_MARKER exit /b 41",
        '>"%CODETERM_TEST_MARKER%" echo helper-ran',
        "exit /b 0",
        "",
      ].join("\r\n"),
      "utf8",
    );

    for (const shell of ["powershell.exe", "pwsh.exe", "cmd.exe"] as const) {
      const marker = path.join(root, `${shell}.marker`);
      fs.rmSync(marker, { force: true });
      const env = { ...process.env, CODETERM_TEST_MARKER: marker };
      const result = shell === "cmd.exe"
        ? spawnSync(shell, ["/d", "/s", "/c", command], { encoding: "utf8", env })
        : spawnSync(
            shell,
            ["-NoProfile", "-NonInteractive", "-Command", command],
            { encoding: "utf8", env },
          );
      if ((result.error as NodeJS.ErrnoException | undefined)?.code === "ENOENT" && shell === "pwsh.exe") {
        console.log("SKIP PowerShell 7 is not installed");
        continue;
      }
      assert.equal(result.error, undefined, `${shell} starts: ${result.error?.message ?? ""}`);
      assert.equal(result.status, 0, `${shell} exits successfully: ${result.stderr}`);
      assert.equal(fs.readFileSync(marker, "utf8").trim(), "helper-ran", `${shell} starts the helper at a spaced path`);
      console.log(`PASS commandWindows invokes the helper through ${shell}`);
    }
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}
