import { describe, expect, it, vi } from "vitest";
import { assignedCommand, parseShellComposition, toolCommandLabel, unwrapShellLauncher } from "./shellComposition";
import { shellFileOperations, SHELL_FILE_LIMITS } from "./shellFileOperations";
import * as shellLexer from "./shellLexer";

const operators = (command: string, argv?: string[]) => parseShellComposition(command, argv)?.segments.map((segment) => segment.operator);

describe("shell composition", () => {
  it("reads a PowerShell or posix assignment as the command it runs and keeps the variable", () => {
    const powershell = "$response = Invoke-WebRequest -Uri https://example.test/api -UseBasicParsing";
    const launched = parseShellComposition(`pwsh -Command ${powershell}`, ["pwsh", "-Command", powershell]);
    expect(launched?.dialect).toBe("powershell");
    expect(launched?.segments[0].argv).toEqual(["Invoke-WebRequest", "-Uri", "https://example.test/api", "-UseBasicParsing"]);
    expect(launched?.segments[0].assignment).toBe("$response");
    expect(launched?.segments[0].text).toBe(powershell);
    expect(parseShellComposition(powershell)?.segments[0].assignment).toBe("$response");
    expect(assignedCommand(powershell)).toEqual({ variable: "$response", label: "Invoke-WebRequest" });

    const posix = parseShellComposition("head=$(git rev-parse HEAD) && echo $head");
    expect(posix?.segments[0].argv).toEqual(["git", "rev-parse", "HEAD"]);
    expect(posix?.segments[0].assignment).toBe("head");
    expect(posix?.segments[1].assignment).toBeUndefined();
    expect(assignedCommand("head=$(git rev-parse HEAD) && echo $head")).toEqual({ variable: "head", label: "git rev-parse +1" });
    expect(parseShellComposition("out=\"$(git status --short)\"")?.segments[0].assignment).toBe("out");
  });

  it("never reads a literal value or an environment prefix as an assigned command", () => {
    for (const command of ["$env:FOO = \"bar\"", "$count = 5", "$name = \"literal\"", "$items = @(1, 2)", "FOO=bar npm test", "plain=value"]) {
      expect(parseShellComposition(command)?.segments[0].assignment).toBeUndefined();
      expect(assignedCommand(command)).toBeNull();
    }
    expect(parseShellComposition("FOO=bar npm test")?.segments[0].argv).toEqual(["FOO=bar", "npm", "test"]);
  });

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

describe("command display labels", () => {
  it.each([
    ["command \"C:\\Users\\dev\\pwsh.exe\" -Command '(Get-Culture).Name'", "(Get-Culture).Name"],
    ["bash -lc 'git status --short'", "git status --short"],
    ['cmd /c "dir & echo ok"', "dir & echo ok"],
  ])("unwraps %s while preserving the full invocation", (command, expected) => {
    const event = { message: { id: "command", type: "tool_call" as const, content: "", toolInput: { command } }, detail: command, summary: command };
    expect(toolCommandLabel(event)).toBe(expected);
    expect(event.detail).toBe(command);
  });

  it("uses parsed command actions ahead of the launcher and preserves source quoting", () => {
    expect(toolCommandLabel({ message: { id: "parsed", type: "tool_call", content: "", toolInput: {
      command: "wrapper --encoded opaque", commandActions: [{ type: "unknown", command: 'Write-Output "hello world"' }],
    } } })).toBe('Write-Output "hello world"');
  });
});

describe("bounded intended shell file operations", () => {
  it("resolves a same-command PowerShell literal path without treating Replace as diff evidence", () => {
    const command = "$p='C:/work/CommandTree.tsx'; $s=Get-Content $p -Raw; $s=$s.Replace('missing','next'); Set-Content $p $s";
    const expected = { operations: [{ action: "write", path: "C:/work/CommandTree.tsx", replacement: {
      path: "C:/work/CommandTree.tsx", fragments: [{ before: "missing", after: "next" }], unsupported: false,
    } }], limited: false };
    expect(shellFileOperations(command)).toEqual(expected);
    expect(shellFileOperations(command, ["pwsh", "-Command", command])).toEqual(expected);
  });

  it("keeps the Snake path and literal fragments through quoted JavaScript and a dynamic remainder", () => {
    const command = `$p='C:/games/snake.html'; $s=Get-Content $p -Raw; $nl=[Environment]::NewLine; $s=$s.Replace('<div id="score">Score: <span>0</span></div>', '<div id="score">Score: <span>0</span> · Best: <span id="best">0</span></div>'); $s=$s.Replace('function tick() { $p=1; }', 'function tick() {' + $nl + 'save();}'); Set-Content -LiteralPath $p $s`;
    const result = shellFileOperations(command);
    expect(result).toEqual({ operations: [{ action: "write", path: "C:/games/snake.html", replacement: {
      path: "C:/games/snake.html", fragments: [{ before: '<div id="score">Score: <span>0</span></div>', after: '<div id="score">Score: <span>0</span> · Best: <span id="best">0</span></div>' }], unsupported: true,
    } }], limited: false });
    expect(shellFileOperations(command)).toBe(result);
  });

  it("decodes only literal pairs including doubled apostrophes and preserves exact order", () => {
    const command = `$p='literal.txt'; $s=Get-Content -LiteralPath $p -Raw; $s=$s.Replace('it''s { $p=1 }','ok'); $s=$s.Replace("old", "two '' quotes"); Set-Content -LiteralPath $p -Value $s`;
    expect(shellFileOperations(command).operations[0].replacement?.fragments).toEqual([
      { before: "it's { $p=1 }", after: "ok" }, { before: "old", after: "two '' quotes" },
    ]);
  });

  it("requires the same local read, self replacement, data variable and final destination", () => {
    const prefix = "$p='same.txt'; $s=Get-Content $p -Raw; $s=$s.Replace('old','new');";
    for (const command of [
      "$p='same.txt'; $s=$s.Replace('old','new'); Set-Content $p $s",
      "$p='same.txt'; $s=Get-Content $p; $s=$s.Replace('old','new'); Set-Content $p $s",
      `${prefix} Set-Content other.txt $s`, `${prefix} Set-Content $p '$s'`,
      `${prefix} Set-Content -Value $p -Path $s`, `${prefix} Set-Content $p $outside`,
      `${prefix} $s='reset'; Set-Content $p $s`, `${prefix} $s=$s.Replace('x','y') + 'tail'; Set-Content $p $s`,
      `${prefix} $s=$other.Replace('x','y'); Set-Content $p $s`, `${prefix} Invoke-Unknown; Set-Content $p $s`,
      `${prefix} $other=Invoke-Unknown; Set-Content $p $s`,
      `${prefix} $s=$s.Replace('x',$(Get-Value)); Set-Content $p $s`,
      `${prefix} $s=$s.Replace('x',"$($p='other')"); Set-Content $p $s`,
      `${prefix} Set-Content $p $s -WhatIf`,
    ]) expect(shellFileOperations(command).operations.every(operation => !operation.replacement), command).toBe(true);
    expect(shellFileOperations(`${prefix} Set-Content $p $s`).operations[0].replacement?.fragments).toEqual([{ before: "old", after: "new" }]);
  });

  it("does not preview dynamic-only replacements, noops or a read without a writer", () => {
    for (const expression of ["$s.Replace('x',$new)", "$s.Replace('x' + $nl,'y')", '$s.Replace("$old","new")', "$s.Replace('x','x')"]) {
      const result = shellFileOperations(`$p='noop.txt'; $s=Get-Content $p -Raw; $s=${expression}; Set-Content $p $s`);
      expect(result.operations).toEqual([{ action: "write", path: "noop.txt" }]);
    }
    expect(shellFileOperations("$p='read.txt'; $s=Get-Content $p -Raw; $s=$s.Replace('old','new')").operations).toEqual([]);
  });

  it("bounds replacement count and UTF-8 fragments while retaining a useful partial prefix", () => {
    const prefix = "$p='bounded.txt'; $s=Get-Content $p -Raw; $s=$s.Replace('first','next');";
    const count = shellFileOperations(`${prefix}${"$s=$s.Replace('a','b');".repeat(SHELL_FILE_LIMITS.replacements)}Set-Content $p $s`);
    expect(count.operations[0].replacement?.fragments).toHaveLength(SHELL_FILE_LIMITS.replacements);
    expect(count.operations[0].replacement?.unsupported).toBe(true);
    expect(count.limited).toBe(true);
    const fragment = shellFileOperations(`${prefix}$s=$s.Replace('${"界".repeat(700)}','x'); Set-Content $p $s`);
    expect(fragment.operations[0].replacement?.fragments).toEqual([{ before: "first", after: "next" }]);
    expect(fragment.operations[0].replacement?.unsupported).toBe(true);
    expect(fragment.limited).toBe(true);
    const total = shellFileOperations(`${prefix}${Array.from({ length: 3 }, (_, index) => `$s=$s.Replace('${"a".repeat(1500)}','${index}${"b".repeat(1499)}');`).join("")}Set-Content $p $s`);
    expect(total.operations[0].replacement?.fragments).toHaveLength(3);
    expect(total.operations[0].replacement?.unsupported).toBe(true);
    expect(total.limited).toBe(true);
  });

  it("preserves single-quoted variable targets as literal filenames", () => {
    const command = "$p='resolved.txt'; Set-Content '$p' 'value'; Set-Content $P 'value'";
    expect(shellFileOperations(command).operations).toEqual([
      { action: "write", path: "$p" }, { action: "write", path: "resolved.txt" },
    ]);
    expect(shellLexer.lexShell("echo hi", "posix")?.[0].rawArgv).toBeUndefined();
  });

  it("recognizes literal writes, appends and file redirects while excluding stream duplication", () => {
    expect(shellFileOperations("Set-Content -LiteralPath 'a b.txt' -Value x; Add-Content notes.txt x; echo x | Out-File -FilePath out.txt -Append").operations).toEqual([
      { action: "write", path: "a b.txt" }, { action: "append", path: "notes.txt" }, { action: "append", path: "out.txt" },
    ]);
    expect(shellFileOperations("printf x > out.txt; printf y >> log.txt; echo x 2>&1; echo x >/dev/null").operations).toEqual([
      { action: "write", path: "out.txt" }, { action: "append", path: "log.txt" },
    ]);
    expect(shellFileOperations("p='local.txt'; printf x > $p").operations).toEqual([{ action: "write", path: "local.txt" }]);
  });

  it("keeps dynamic, stale, wildcard and substitution targets unknown", () => {
    for (const command of [
      "Set-Content $outside x",
      "$p='old.txt'; $p=Get-Path; Set-Content $p x",
      "$p=Get-Path; Set-Content $p x",
      "$p='a'+'b'; Set-Content $p x",
      "$p='old.txt'; Set-Variable p changed.txt; Set-Content $p x",
      "$p='old.txt' && Set-Content $p x",
      "$p='old.txt'; Set-Content -Value $(Get-Value) -Path $p",
      "Set-Content '*.txt' x",
      "Set-Content -Unknown guess.txt x",
      "echo x > $(target)",
    ]) {
      expect(shellFileOperations(command).operations).toEqual([{ action: "write", path: null }]);
    }
    expect(shellFileOperations("Set-Content $p x").operations[0].path).toBeNull();
  });

  it("makes no write claim for reads, replacement expressions or unsupported embedded programs", () => {
    for (const command of ["Get-Content a.txt -Raw", "cat a.txt", "$s=$s.Replace('x','y')", "python -c \"from pathlib import Path; Path('a').write_text('x')\"", "node -e \"require('fs').writeFileSync('a','x')\"", "if test x; then echo x > a; fi"]) {
      expect(shellFileOperations(command).operations).toEqual([]);
    }
  });

  it("caps input bytes and argv before parsing or retaining a key", () => {
    const lexer = vi.spyOn(shellLexer, "lexShell");
    try {
      for (const command of ["x".repeat(SHELL_FILE_LIMITS.inputBytes + 1), "界".repeat(SHELL_FILE_LIMITS.inputBytes / 2)]) {
        expect(shellFileOperations(command)).toEqual({ operations: [], limited: true });
      }
      expect(shellFileOperations("echo", Array(SHELL_FILE_LIMITS.argv + 1).fill("x")).limited).toBe(true);
      expect(shellFileOperations("echo", ["x".repeat(SHELL_FILE_LIMITS.inputBytes)]).limited).toBe(true);
      expect(lexer).not.toHaveBeenCalled();
    } finally { lexer.mockRestore(); }
  });

  it("caps operations, variables, paths, tokens and segments", () => {
    const writes = Array.from({ length: 12 }, (_, index) => `Set-Content file${index}.txt x`).join(";");
    expect(shellFileOperations(writes).operations).toHaveLength(SHELL_FILE_LIMITS.operations);
    expect(shellFileOperations(writes).limited).toBe(true);
    const variables = Array.from({ length: 17 }, (_, index) => `$p${index}='file${index}.txt'`).join(";");
    const result = shellFileOperations(`${variables}; Set-Content $p16 x`);
    expect(result.operations).toEqual([{ action: "write", path: null }]);
    expect(result.limited).toBe(true);
    expect(shellFileOperations(`Set-Content '${"a".repeat(SHELL_FILE_LIMITS.path + 1)}' x`).operations[0].path).toBeNull();
    expect(shellFileOperations(`echo ${"x ".repeat(SHELL_FILE_LIMITS.tokens)} > tail.txt`).operations).toEqual([]);
    expect(shellFileOperations(`${"echo x;".repeat(SHELL_FILE_LIMITS.segments)}echo x > tail.txt`)).toEqual({ operations: [], limited: true });
  });

  it("reuses immutable bounded analyses and evicts old command keys", () => {
    const command = "echo cache > reusable-file.txt";
    const lexer = vi.spyOn(shellLexer, "lexShell");
    try {
      const first = shellFileOperations(command);
      const parses = lexer.mock.calls.filter(call => call[2] === true).length;
      expect(parses).toBe(1);
      expect(shellFileOperations(command)).toBe(first);
      expect(lexer.mock.calls.filter(call => call[2] === true)).toHaveLength(parses);
      expect(Object.isFrozen(first.operations)).toBe(true);
      for (let index = 0; index < SHELL_FILE_LIMITS.cache; index++) shellFileOperations(`echo ${index} > eviction-${index}.txt`);
      expect(shellFileOperations(command)).not.toBe(first);
      const large = `echo '${"x".repeat(4000)}' > byte-budget.txt`;
      const retained = shellFileOperations(large);
      for (let index = 0; index < 40; index++) shellFileOperations(`echo '${"y".repeat(4000)}' > budget-${index}.txt`);
      expect(shellFileOperations(large)).not.toBe(retained);
    } finally { lexer.mockRestore(); }
  });

  it("accounts replacement fragment strings in the cache budget", () => {
    const pairs = ["ab", "cd"].map(pair => `$s=$s.Replace('${pair[0].repeat(1500)}','${pair[1].repeat(1500)}');`).join("");
    const command = (index: number) => `$p='fragment-cache-${index}.txt'; $s=Get-Content $p -Raw; ${pairs} Set-Content $p $s`;
    const first = shellFileOperations(command(0));
    expect(first.operations[0].replacement?.fragments).toHaveLength(2);
    expect(Object.isFrozen(first.operations[0].replacement?.fragments)).toBe(true);
    expect(Object.isFrozen(first.operations[0].replacement?.fragments[0])).toBe(true);
    expect(shellFileOperations(command(0))).toBe(first);
    for (let index = 1; index <= 12; index++) shellFileOperations(command(index));
    expect(shellFileOperations(command(0))).not.toBe(first);
  });
});
