import { describe, expect, it } from "vitest";
import { parseShellComposition, unwrapShellLauncher } from "./shellComposition";

const operators = (command: string, argv?: string[]) => parseShellComposition(command, argv)?.segments.map((segment) => segment.operator);

describe("shell composition", () => {
  it("splits the Codex pwsh pipeline into a CodeTerm segment piped into an inline python script", () => {
    const inner = [
      "codeterm mem secret get --name almaz-admin-clerk-secret-key | python -c \"import sys,json,httpx",
      "key = sys.stdin.read().strip()",
      "r = httpx.get('https://api.clerk.com/v1/users', headers={'Authorization': 'Bearer ' + key})",
      "print(json.dumps(r.json()[:1], indent=2))\"",
    ].join("\n");
    const argv = ["C:\\Users\\dev\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe", "-Command", inner];
    const composition = parseShellComposition(argv.join(" "), argv);
    expect(composition?.dialect).toBe("powershell");
    expect(composition?.launcher).toBe("pwsh");
    expect(composition?.segments).toHaveLength(2);
    const [secret, script] = composition!.segments;
    expect(secret.operator).toBe("pipe");
    expect(secret.argv).toEqual(["codeterm", "mem", "secret", "get", "--name", "almaz-admin-clerk-secret-key"]);
    expect(secret.text).toBe("codeterm mem secret get --name almaz-admin-clerk-secret-key");
    expect(script.operator).toBeNull();
    expect(script.argv[0]).toBe("python");
    expect(script.script?.language).toBe("python");
    expect(script.script?.lineCount).toBe(4);
    expect(script.script?.body.startsWith("import sys,json,httpx\nkey = sys.stdin.read()")).toBe(true);
  });

  it("unwraps the same launcher from the command string when no argv is supplied", () => {
    const composition = parseShellComposition("pwsh.exe -NoProfile -Command \"codeterm docs API | Select-Object -First 3\"");
    expect(composition?.dialect).toBe("powershell");
    expect(composition?.segments.map((segment) => segment.argv[0])).toEqual(["codeterm", "Select-Object"]);
  });

  it("parses bash -lc chains joined by && ; and ||", () => {
    const composition = parseShellComposition("bash -lc 'cd /repo && npm test; git status || echo failed'");
    expect(composition?.launcher).toBe("bash");
    expect(composition?.segments.map((segment) => segment.text)).toEqual(["cd /repo", "npm test", "git status", "echo failed"]);
    expect(operators("bash -lc 'cd /repo && npm test; git status || echo failed'")).toEqual(["and", "then", "or", null]);
  });

  it("keeps quoted pipes and semicolons inside one segment", () => {
    const composition = parseShellComposition("echo \"a | b; c && d\" && printf 'x;y|z' | tr ';' ,");
    expect(composition?.segments.map((segment) => segment.argv)).toEqual([
      ["echo", "a | b; c && d"],
      ["printf", "x;y|z"],
      ["tr", ";", ","],
    ]);
    expect(composition?.segments.map((segment) => segment.operator)).toEqual(["and", "pipe", null]);
  });

  it("treats newlines as statement breaks and keeps redirects out of argv", () => {
    const composition = parseShellComposition("cd /d/Developer/codeterm\ncodeterm agent report --status in_review --text \"line one\nline two\" 2>&1 | tail -3");
    expect(composition?.segments.map((segment) => segment.operator)).toEqual(["then", "pipe", null]);
    expect(composition?.segments[1].argv).toEqual(["codeterm", "agent", "report", "--status", "in_review", "--text", "line one\nline two"]);
    expect(composition?.segments[1].redirects).toEqual(["2>&1"]);
  });

  it("parses a PowerShell pipeline with script blocks and nested quotes", () => {
    const argv = ["pwsh", "-NoProfile", "-Command", "Get-ChildItem -Path 'C:\\it''s here' | Where-Object { $_.Name -like \"*|*\" } | Select-Object -First 3; Write-Output \"done `\"ok`\"\""];
    const composition = parseShellComposition(argv.join(" "), argv);
    expect(composition?.segments.map((segment) => segment.operator)).toEqual(["pipe", "pipe", "then", null]);
    expect(composition?.segments[0].argv).toEqual(["Get-ChildItem", "-Path", "C:\\it's here"]);
    expect(composition?.segments[1].argv).toEqual(["Where-Object", "{ $_.Name -like \"*|*\" }"]);
    expect(composition?.segments[3].argv).toEqual(["Write-Output", "done \"ok\""]);
  });

  it("reads a heredoc as the script payload of its interpreter", () => {
    const command = "cd /repo && python3 - <<'PY' | tail -1\nimport json\nprint(json.dumps({\"a\": 1}))\nPY\ngit status";
    const composition = parseShellComposition(command);
    expect(composition?.segments.map((segment) => segment.text)).toEqual(["cd /repo", "python3 - <<'PY'", "tail -1", "git status"]);
    expect(composition?.segments.map((segment) => segment.operator)).toEqual(["and", "pipe", "then", null]);
    expect(composition?.segments[1].script).toEqual({ language: "python", body: "import json\nprint(json.dumps({\"a\": 1}))", lineCount: 2 });
  });

  it("reads a PowerShell here-string as one argument", () => {
    const argv = ["pwsh", "-Command", "@'\nline | one\nline; two\n'@ | Set-Content notes.txt"];
    const composition = parseShellComposition(argv.join(" "), argv);
    expect(composition?.segments.map((segment) => segment.argv)).toEqual([["line | one\nline; two"], ["Set-Content", "notes.txt"]]);
  });

  it("recognizes inline interpreters and nested launchers as scripts", () => {
    expect(parseShellComposition("node -e \"console.log(1)\" && ls")?.segments[0].script).toEqual({ language: "javascript", body: "console.log(1)", lineCount: 1 });
    const nested = parseShellComposition("git fetch && pwsh -NoProfile -Command \"Get-Date; Get-Location\"");
    expect(nested?.segments[1].script).toEqual({ language: "powershell", body: "Get-Date; Get-Location", lineCount: 1 });
  });

  it("unwraps cmd /c and nested launchers", () => {
    expect(unwrapShellLauncher("cmd /c \"dir & echo ok\"")).toEqual({ command: "dir & echo ok", dialect: "cmd", launcher: "cmd" });
    expect(operators("cmd /c \"dir & echo ok\"")).toEqual(["then", null]);
    expect(unwrapShellLauncher("x", ["pwsh", "-Command", "bash -lc 'npm test'"])).toEqual({ command: "npm test", dialect: "posix", launcher: "pwsh" });
    expect(unwrapShellLauncher("npm test")).toEqual({ command: "npm test", dialect: "posix" });
  });

  it("returns null for ambiguous or unsupported syntax", () => {
    for (const command of [
      "echo \"unterminated | tail",
      "echo 'unterminated",
      "python3 - <<PY\nprint(1)\n",
      "npm test &&",
      "a | | b",
      "(cd sub && make)",
      "sleep 10 &",
      "if true; then echo yes; fi",
      "cat <(ls)",
      "echo `date",
      "",
    ]) {
      expect(parseShellComposition(command), command).toBeNull();
    }
    expect(parseShellComposition("pwsh -Command \"Get-Item (Get-Location\"", ["pwsh", "-Command", "Get-Item (Get-Location"])).toBeNull();
  });

  it("never throws on arbitrary input", () => {
    const alphabet = "ab |&;<>()'\"`$@{}\\#\n\t-=";
    let seed = 7;
    for (let run = 0; run < 400; run++) {
      let command = "";
      for (let i = 0; i < 24; i++) {
        seed = (seed * 1103515245 + 12345) % 2147483648;
        command += alphabet[seed % alphabet.length];
      }
      expect(() => parseShellComposition(command)).not.toThrow();
      expect(() => parseShellComposition(command, ["pwsh", "-Command", command])).not.toThrow();
    }
  });
});
