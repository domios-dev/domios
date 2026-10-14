import { commandLabel } from "../src/commandLabel";

function eq(actual: unknown, expected: unknown, label: string): void {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(label + ": " + JSON.stringify(actual));
  console.log("  ✓ " + label);
}

const pwsh = "C:\\Users\\dev\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe";

eq(commandLabel([pwsh, "-NoLogo", "-NoProfile", "-Command", "git status --short"]), "git status --short", "pwsh argv from the screenshot");
eq(commandLabel(`"${pwsh.replace(/\\/g, "\\\\")}" -NoLogo -NoProfile -Command "git status --short"`), "git status --short", "pwsh string with JSON-escaped backslashes");
eq(commandLabel(`"${pwsh}" -Command git   log -1`), "git log -1", "pwsh string collapses whitespace");
eq(commandLabel(["powershell.exe", "-c", "dir"]), "dir", "powershell -c");
eq(commandLabel(["/bin/bash", "-lc", "pnpm test"]), "pnpm test", "bash -lc");
eq(commandLabel("/usr/bin/zsh -c 'ls -la'"), "ls -la", "zsh -c quoted string");
eq(commandLabel(["cmd.exe", "/c", "echo hi"]), "echo hi", "cmd /c argv");
eq(commandLabel("cmd /d /s /c \"dir\""), "dir", "cmd /c string");
eq(commandLabel(["C:\\Program Files\\Git\\cmd\\git.exe", "status", "--short"]), "git status --short", "plain argv drops the directory");
eq(commandLabel(["rg", "two words"]), 'rg "two words"', "plain argv keeps spaced arguments readable");
eq(commandLabel('"git status"'), "git status", "JSON-quoted single string");
eq(commandLabel("git status"), "git status", "plain string");
eq(commandLabel(["pwsh", "-NoProfile"]), "pwsh -NoProfile", "launcher without a command stays as is");
eq(commandLabel(["sh", "-c", "x ".repeat(200)]).length, 160, "caps at 160 characters");
eq(commandLabel(undefined), "", "non-command input is empty");
