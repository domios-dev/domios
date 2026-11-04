import React, { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { reduceToolEvents, type NormalizedChatMessage, type ToolEvent } from "@codeterm/chat-core";
import { toolCommandShape } from "../codeterm/commandShape";
import { ToolCallBubble } from "../ToolCallBubble";
import { ChatBlockHostProvider } from "../blocks/ChatBlockHost";
import { CommandGroupCard } from "./CommandGroupCard";
import { act, render } from "./cardTestSetup";
import * as shellLexer from "../../../chat-core/src/shellLexer";

const CODEX_SESSION = "01a09a2f-b88c-79b3-a9dd-83f8741f6810";
const CODEX_PIPELINE = [
  "codeterm mem secret get --name almaz-admin-clerk-secret-key | python -c \"import sys,json,httpx",
  "key = sys.stdin.read().strip()",
  "r = httpx.get('https://api.clerk.com/v1/users', headers={'Authorization': 'Bearer ' + key})",
  "print(json.dumps(r.json()[:1], indent=2))\"",
].join("\n");
const CODEX_ARGV = ["C:\\Program Files\\PowerShell\\7\\pwsh.exe", "-Command", CODEX_PIPELINE];
const SPEC_CHAIN = "codeterm spec section update --help ; codeterm plan task update --help";
const PLAN_CHAIN = "codeterm plan doctor ; codeterm plan decision list ; codeterm task show ; codeterm task show";

/** Projection of the rollout's exec custom_tool_call, its output, and the CommandExecution item that echoes it. */
function codexRollout(): NormalizedChatMessage[] {
  const session = { sourceSessionId: CODEX_SESSION, provider: "codex" };
  return [
    {
      id: "call_exec", type: "tool_call", content: "", toolName: "exec", toolId: "call_exec", toolKind: "command",
      toolSummary: CODEX_PIPELINE, toolDetail: CODEX_PIPELINE, toolTitle: "codeterm mem",
      toolInput: { raw: `text(await tools.exec_command({cmd: ${JSON.stringify(CODEX_PIPELINE)}}));` }, ...session,
    },
    { id: "call_exec:result", type: "tool_result", content: "Script completed\nOutput:\n[{\"id\": \"user_1\"}]", toolId: "call_exec", ...session },
    {
      id: "activity-cmd_4333", type: "tool_call", content: "", toolName: "command_execution", toolId: "cmd_4333", toolKind: "command",
      toolSummary: CODEX_ARGV.join(" "), toolDetail: CODEX_ARGV.join(" "), toolTitle: "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
      toolInput: { command: CODEX_ARGV.join(" "), argv: CODEX_ARGV }, toolEcho: true, toolError: false,
      toolResult: "[{\"id\": \"user_1\"}]", toolExitCode: 0, toolDurationMs: 1830, ...session,
    },
  ];
}

/** Projection of a Claude jsonl Bash tool_use with its description and a paired tool_result. */
function claudeBash(command: string, description?: string): NormalizedChatMessage[] {
  const session = { sourceSessionId: "claude-session", provider: "claude" };
  return [
    {
      id: "toolu_bash", type: "tool_call", content: "", toolName: "Bash", toolId: "toolu_bash", toolKind: "command",
      toolSummary: command, toolDetail: command, ...(description ? { toolDescription: description } : {}),
      toolInput: { command, ...(description ? { description } : {}) }, toolFlowId: "msg_1", toolCallSourceId: "assistant-1", ...session,
    },
    {
      id: "toolu_bash:result", type: "tool_result", content: "3", toolId: "toolu_bash", toolCallSourceId: "assistant-1",
      toolPairing: "exact", toolExitCode: 0, toolDurationMs: 420, ...session,
    },
  ];
}

/** Projection of a Codex exec_command call running a chain, with its output. */
function codexChain(id: string, command: string): NormalizedChatMessage[] {
  const session = { sourceSessionId: CODEX_SESSION, provider: "codex" };
  return [
    {
      id, type: "tool_call", content: "", toolName: "exec_command", toolId: id, toolKind: "command",
      toolSummary: command, toolDetail: command, toolTitle: command.split(" ").slice(0, 2).join(" "), toolInput: { cmd: command }, ...session,
    },
    { id: `${id}:result`, type: "tool_result", content: "", toolResult: "doctor: ok", toolId: id, toolExitCode: 0, toolDurationMs: 481, ...session },
  ];
}

function singleEvent(messages: NormalizedChatMessage[]): ToolEvent {
  const items = reduceToolEvents(messages);
  expect(items).toHaveLength(1);
  if (items[0].kind !== "tool") throw new Error("expected one tool event");
  return items[0].event;
}

async function click(element: Element | null | undefined) {
  if (!element) throw new Error("missing clickable element");
  await act(async () => (element as HTMLElement).click());
}

function lines(scope: ParentNode): HTMLElement[] {
  return [...scope.querySelectorAll<HTMLElement>('[data-testid="chat-command-tree-line"]')];
}

function leads(scope: ParentNode): string[] {
  return lines(scope).map((line) => line.querySelector('[data-testid="chat-command-tree-lead"]')?.textContent ?? "");
}

function operators(scope: ParentNode): string[] {
  return [...scope.querySelectorAll<HTMLElement>('[data-testid="chat-command-tree-operator"]')].map((node) => node.dataset.operator ?? "");
}

function Stateful({ event, initiallyExpanded = false }: { event: ToolEvent; initiallyExpanded?: boolean }) {
  const [expanded, setExpanded] = useState(initiallyExpanded);
  return <ToolCallBubble event={event} expanded={expanded} onToggle={setExpanded} />;
}

describe("compound command group", () => {
  it("discloses supplied aggregate partial output once at whole-command scope", async () => {
    const event = {...singleEvent(codexChain("opaque-partial-group",SPEC_CHAIN)),status:"pending" as const,result:"opaque-partial-output"};
    const shape = toolCommandShape(event);
    expect(shape.kind).toBe("group");
    if (shape.kind !== "group") throw new Error("fixture must be a command group");
    const closed = await render(<CommandGroupCard event={event} rows={shape.rows} expanded={false} onToggle={() => {}} />);
    expect(closed.querySelector('[aria-label="Command output"]')?.hasAttribute("hidden")).toBe(true);
    expect(closed.textContent).not.toContain(event.result);
    const open = await render(<ChatBlockHostProvider value={{ renderMenu: ({items}) => <>{items.map(item =>
      <button key={item.label} onClick={item.onSelect}>{item.label}</button>)}</> }}>
      <CommandGroupCard event={event} rows={shape.rows} expanded onToggle={() => {}} />
    </ChatBlockHostProvider>);
    expect(open.textContent).not.toContain(event.result);
    const details = [...open.querySelectorAll("button")].find(button => button.textContent === "Details")!;
    await act(async () => details.click());
    expect(open.querySelector('[aria-label="Command output"]')?.textContent).toContain(event.result);
    expect(open.textContent?.split(event.result)).toHaveLength(2);
    expect(open.querySelector('[data-testid="chat-command-group-status"]')?.getAttribute("aria-label")).toBe("pending");
    expect(open.querySelector('[data-testid="chat-command-tree-line"] [aria-label="pending"]')).toBeNull();
  });

  it("renders the Codex pwsh pipeline once as one box with a CodeTerm line and a python script line", async () => {
    const host = await render(<ToolCallBubble event={singleEvent(codexRollout())} expanded={false} onToggle={() => {}} />);
    const groups = host.querySelectorAll('[data-testid="chat-command-group"]');
    expect(groups).toHaveLength(1);
    const group = groups[0];
    expect(lines(group).map((line) => line.dataset.rowRole)).toEqual(["codeterm", "script"]);
    expect(operators(group)).toEqual([]);
    expect(group.querySelector('[data-testid="chat-command-group-title"]')?.textContent).toBe("domios mem secret get +1");

    const codetermLine = lines(group)[0];
    expect(codetermLine.querySelector('[data-testid="chat-tool-call"]')).toBeNull();
    expect(codetermLine.querySelector(".lucide-brain")).not.toBeNull();
    expect(codetermLine.textContent).toContain("mem");

    expect(host.querySelectorAll('[data-testid="toolcard-exit-code"]')).toHaveLength(1);
    expect(host.querySelectorAll('[data-testid="toolcard-duration"]')).toHaveLength(1);
    expect(group.querySelector('[data-testid="chat-command-group-toggle"] [data-testid="toolcard-exit-code"]')?.textContent).toBe("exit 0");
  });

  it("expands an inline script line into a highlighted body with its language and line count", async () => {
    const host = await render(<ToolCallBubble event={singleEvent(codexRollout())} expanded={false} onToggle={() => {}} />);
    const scriptLine = lines(host).find((line) => line.dataset.rowRole === "script");
    expect(scriptLine?.textContent).toContain("python");
    expect(scriptLine?.textContent).toContain("4 lines");
    expect(host.querySelector('[data-testid="chat-command-script-body"]')).toBeNull();
    await click(scriptLine?.querySelector("button"));
    const body = host.querySelector<HTMLElement>('[data-testid="chat-command-script-body"]');
    expect(body?.dataset.language).toBe("python");
    expect(body?.textContent).toContain("import sys,json,httpx");
    expect(body?.querySelector(".token")).not.toBeNull();
  });

  it("shows the parsed command tree without a raw block or a combined output dump", async () => {
    const host = await render(<ToolCallBubble event={singleEvent(codexRollout())} expanded onToggle={() => {}} />);
    expect(host.querySelector('[data-testid="chat-command-group"] [data-testid="raw-command-disclosure"]')).toBeNull();
    expect(host.querySelector('[data-testid="message-raw-content"]')).toBeNull();
    expect(host.querySelectorAll('[data-testid="toolcard-result"]')).toHaveLength(0);
    expect(lines(host).length).toBeGreaterThan(0);
  });

  it("titles a Claude Bash compound by its description and lists segments as a plain tree", async () => {
    const command = "gh issue list --state open --json number && gh pr list || echo offline; git status";
    const host = await render(<ToolCallBubble event={singleEvent(claudeBash(command, "Count open work"))} expanded onToggle={() => {}} />);
    const group = host.querySelector('[data-testid="chat-command-group"]');
    expect(group?.querySelector('[data-testid="chat-command-group-title"]')?.textContent).toBe("Count open work");
    expect(operators(host)).toEqual([]);
    expect(lines(host).map((line) => line.textContent?.match(/&&|\|\||;/))).toEqual([null, null, null, null]);
    expect(lines(host).map((line) => line.dataset.rowRole)).toEqual(["command", "command", "command", "command"]);
    expect(host.querySelectorAll('[data-testid="toolcard-exit-code"]')).toHaveLength(1);
  });

  it("reads a Claude heredoc as a script line", async () => {
    const command = "cd /repo && python3 - <<'PY'\nimport json\nprint(json.dumps({\"ok\": True}))\nPY";
    const host = await render(<ToolCallBubble event={singleEvent(claudeBash(command))} expanded={false} onToggle={() => {}} />);
    expect(lines(host).map((line) => line.dataset.rowRole)).toEqual(["command", "script"]);
    expect(lines(host)[1].textContent).toContain("2 lines");
  });

  it("falls back to today's single command row when the parser returns null", async () => {
    const command = "echo \"unterminated | python -c 'print(1)'";
    const event = singleEvent(claudeBash(command));
    expect(toolCommandShape(event).kind).toBe("single");
    const host = await render(<ToolCallBubble event={event} expanded={false} onToggle={() => {}} />);
    expect(host.querySelector('[data-testid="chat-command-group"]')).toBeNull();
    expect(host.querySelector('[data-testid="chat-command-tree"]')).toBeNull();
    expect(host.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(1);
    expect(host.querySelector(".lucide-terminal")).not.toBeNull();
  });

  it("reads the command from the source input, not multi-line presentation detail", async () => {
    const event = singleEvent([
      {
        id: "cb-call", type: "tool_call", content: "", provider: "lmstudio", toolName: "exec", toolId: "cb-call", toolKind: "command",
        toolInput: { cmd: "npm test -- --runInBand", cwd: "/repo/app" },
        toolSummary: "npm test -- --runInBand", toolDetail: "npm test -- --runInBand\ncwd /repo/app",
      },
      { id: "cb-result", type: "tool_result", content: "Exit code: 0\nTests passed", toolResult: "Exit code: 0\nTests passed", toolId: "cb-call" },
    ]);
    expect(toolCommandShape(event).kind).toBe("single");
    const host = await render(<ToolCallBubble event={event} expanded={false} onToggle={() => {}} />);
    expect(host.querySelector('[data-testid="chat-command-group"]')).toBeNull();
    expect(host.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(1);
  });

  it("memoizes the shape per call instead of re-parsing", () => {
    const event = singleEvent(codexRollout());
    expect(toolCommandShape({ ...event })).toBe(toolCommandShape(event));
  });
});

describe("compact command tree", () => {
  it("prints one tree line per segment of the two-segment CodeTerm help chain, in order, without operator glyphs", async () => {
    const host = await render(<ToolCallBubble event={singleEvent(codexChain("chain-spec", SPEC_CHAIN))} expanded={false} onToggle={() => {}} />);
    expect(leads(host)).toEqual(["spec section update", "plan task update"]);
    expect(lines(host).map((line) => line.querySelector('[data-testid="chat-command-tree-object"]')?.textContent)).toEqual(["help", "help"]);
    expect(operators(host)).toEqual([]);
    expect(lines(host).map((line) => line.textContent?.charAt(0))).toEqual(["├", "└"]);
    expect(lines(host)[1].textContent).not.toContain(";");
    expect(host.querySelector('[data-testid="chat-command-group-operator"]')).toBeNull();
    for (const line of lines(host)) {
      expect([...line.children].map((child) => child.tagName)).toEqual(["BUTTON"]);
      expect(line.querySelector('[data-testid="chat-tool-call"]')).toBeNull();
    }
  });

  it("caps the four-segment chain with +N more, and prints every CodeTerm label once", async () => {
    const event = singleEvent(codexChain("chain-plan", PLAN_CHAIN));
    const host = await render(<Stateful event={event} />);
    expect(leads(host)).toEqual(["plan doctor", "plan decision list", "task show"]);
    expect(operators(host)).toEqual([]);
    const more = host.querySelector('[data-testid="chat-command-tree-more"]');
    expect(more?.textContent).toContain("Show all 1 remaining step");

    await click(more);
    expect(host.querySelector('[data-testid="chat-command-tree-more"]')).toBeNull();
    expect(leads(host)).toEqual(["plan doctor", "plan decision list", "task show", "task show"]);
    expect(operators(host)).toEqual([]);
    await click(host.querySelector('[data-testid="chat-command-group-toggle"]'));
    expect(leads(host)).toEqual(["plan doctor", "plan decision list", "task show"]);
    expect(host.querySelector('[data-testid="chat-command-tree-more"]')).not.toBeNull();
    await click(host.querySelector('[data-testid="chat-command-tree-more"]'));
    expect(leads(host)).toHaveLength(4);
    for (const line of lines(host)) {
      const lead = line.querySelector('[data-testid="chat-command-tree-lead"]')?.textContent ?? "";
      expect(line.textContent?.split(lead).length).toBe(2);
      expect(line.querySelector('[data-testid="chat-command-tree-object"]')).toBeNull();
    }
  });

  it("opens only the clicked segment's detail inline", async () => {
    const host = await render(<ToolCallBubble event={singleEvent(codexChain("chain-open", PLAN_CHAIN))} expanded onToggle={() => {}} />);
    expect(host.querySelector('[data-testid="chat-command-tree-detail"]')).toBeNull();
    await click(lines(host)[1].querySelector("button"));
    const details = host.querySelectorAll('[data-testid="chat-command-tree-detail"]');
    expect(details).toHaveLength(1);
    expect(lines(host)[1].contains(details[0])).toBe(true);
    expect(details[0].textContent).toContain("codeterm plan decision list");
    expect(lines(host).map((line) => line.querySelector("button")?.getAttribute("aria-expanded"))).toEqual(["false", "true", "false", "false"]);

    await click(lines(host)[3].querySelector("button"));
    expect(host.querySelectorAll('[data-testid="chat-command-tree-detail"]')).toHaveLength(1);
    expect(lines(host)[3].querySelector('[data-testid="chat-command-tree-detail"]')).not.toBeNull();
  });

  it("heads the group with a short summary instead of the raw chain and keeps one outcome", async () => {
    const host = await render(<ToolCallBubble event={singleEvent(codexChain("chain-head", PLAN_CHAIN))} expanded onToggle={() => {}} />);
    const title = host.querySelector('[data-testid="chat-command-group-title"]')?.textContent ?? "";
    expect(title).not.toBe(PLAN_CHAIN);
    expect(title).not.toContain(";");
    expect(title).toBe("domios plan doctor +3");
    expect(host.querySelectorAll('[data-testid="toolcard-exit-code"]')).toHaveLength(1);
    expect(host.querySelectorAll('[data-testid="toolcard-duration"]')).toHaveLength(1);
    expect(host.querySelector('[data-testid="raw-command-disclosure"]')).toBeNull();
    expect(lines(host)).toHaveLength(4);
  });
});

describe("shell file operation visibility", () => {
  it("opens known append content without inventing a previous file version", async () => {
    const command = "Add-Content -LiteralPath 'notes.txt' -Value 'second line'";
    const event = singleEvent(codexChain("append-preview", command));
    const host = await render(<ToolCallBubble event={event} expanded={false} onToggle={() => {}} />);
    expect(host.querySelector('[data-testid="shell-replacement-fragments"]')).toBeNull();
    await click(host.querySelector('button[aria-label="Show append preview for notes.txt"]'));
    const content = host.querySelector('[data-testid="shell-replacement-fragments"]');
    expect(content?.textContent).toContain("second line");
    expect(content?.textContent).not.toContain("Before");
  });
  it("discloses literal command fragments inside the command and preserves call-local expansion", async () => {
    const command = `$p='C:/games/snake.html'; $s=Get-Content $p -Raw; $nl=[Environment]::NewLine; $s=$s.Replace('<div id="score">Score: <span>0</span></div>', '<div id="score">Score: <span>0</span> · Best: <span id="best">0</span></div>'); $s=$s.Replace('function tick() { $p=1; }', 'function tick() {' + $nl + 'save();}'); Set-Content -LiteralPath $p $s`;
    const base = singleEvent(codexChain("snake-replacement-preview", command));
    const event = { ...base, message: { ...base.message, toolInput: { command, argv: ["pwsh", "-Command", command] } } };
    const host = await render(<Stateful event={event} />);
    const operations = host.querySelector('ul[aria-label="Intended file operations"]');
    const toggle = operations?.querySelector('button[aria-label="Show replacement preview for snake.html"]') ?? null;
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    expect(operations?.textContent).toContain("snake.html1 replacement");
    expect(operations?.textContent).not.toContain("C:/games");
    expect(toggle?.hasAttribute("title")).toBe(false);
    expect(host.querySelector('[data-testid="chat-command-group"]')?.contains(operations)).toBe(true);
    expect(host.querySelector('[data-testid="shell-replacement-fragments"]')).toBeNull();
    expect(host.querySelector('[data-testid="file-diff-card"]')).toBeNull();
    await act(async () => (toggle as HTMLElement).focus());
    expect(document.querySelector('[role="tooltip"]')?.textContent).toBe("C:/games/snake.html");
    await click(toggle);
    const fragments = host.querySelector('[data-testid="shell-replacement-fragments"]');
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect([...(fragments?.querySelectorAll("pre") ?? [])].map(node => node.textContent)).toEqual([
      '<div id="score">Score: <span>0</span></div>', '<div id="score">Score: <span>0</span> · Best: <span id="best">0</span></div>',
    ]);
    expect(fragments?.querySelector('[role="note"][aria-label="Partial replacement preview"]')).not.toBeNull();
    expect(fragments?.textContent).not.toContain("Get-Content");
    await click(host.querySelector('[data-testid="chat-command-group-toggle"]'));
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    await click(lines(host)[0].querySelector("button"));
    expect(lines(host)[0].querySelector('[data-testid="chat-command-tree-detail"]')?.textContent).toBe("$p='C:/games/snake.html'");
    const remounted = await render(<Stateful event={{ ...event, status: "success" }} />);
    expect(remounted.querySelector('button[aria-label="Hide replacement preview for snake.html"]')?.getAttribute("aria-expanded")).toBe("true");
    const otherSession = await render(<Stateful event={{ ...event, message: { ...event.message, sourceSessionId: "other-session" } }} />);
    expect(otherSession.querySelector('[data-testid="shell-replacement-fragments"]')).toBeNull();
    await click(toggle);
    expect(host.querySelector('[data-testid="shell-replacement-fragments"]')).toBeNull();
  });

  it("shows intended command-local targets, reuses analysis on render, and preserves truthful diff disclosure", async () => {
    const command = "$p='C:/work/preview.ts'; $s=Get-Content $p -Raw; $s=$s.Replace('unmatched','next'); Set-Content $p $s";
    const base = singleEvent(codexChain("file-operation-main", command));
    const main: ToolEvent = { ...base, message: { ...base.message, toolInput: { command, argv: ["pwsh", "-Command", command] } } };
    const lexer = vi.spyOn(shellLexer, "lexShell");
    try {
      function Preview() {
        const [expanded, setExpanded] = useState(false);
        return <ToolCallBubble event={{ ...main }} expanded={expanded} onToggle={setExpanded} />;
      }
      const host = await render(<Preview />);
      const operations = host.querySelector('ul[aria-label="Intended file operations"]');
      expect(operations?.children).toHaveLength(1);
      expect(operations?.textContent).toContain("preview.ts");
      expect(host.querySelector('[data-testid="file-diff-card"]')).toBeNull();
      const parses = lexer.mock.calls.filter(call => call[2] === true).length;
      expect(parses).toBe(1);
      const groupToggle = host.querySelector('[data-testid="chat-command-group-toggle"]');
      expect(groupToggle?.getAttribute("aria-expanded")).toBe("false");
      await click(groupToggle);
      expect(groupToggle?.getAttribute("aria-expanded")).toBe("true");
      await click(lines(host)[3].querySelector("button"));
      expect(lines(host)[3].querySelector('[data-testid="chat-command-tree-detail"]')?.textContent).toContain("Set-Content $p $s");
      expect(lexer.mock.calls.filter(call => call[2] === true)).toHaveLength(parses);
      expect(host.querySelectorAll('ul[aria-label="Intended file operations"]')).toHaveLength(1);

      for (const status of ["pending", "success", "error", "interrupted", "unresolved"] as const) {
        const failed = await render(<ToolCallBubble event={{ ...main, id: `write-${status}`, status }} expanded={false} onToggle={() => {}} />);
        expect(failed.querySelector('ul[aria-label="Intended file operations"]')?.children).toHaveLength(1);
        expect(failed.querySelector('[data-testid="file-diff-card"]')).toBeNull();
      }
      const unknown = await render(<ToolCallBubble event={singleEvent(codexChain("unknown-file", "Set-Content $outside x"))} expanded={false} onToggle={() => {}} />);
      // A write into a target that cannot be resolved has nothing to show, so no row and no list render.
      expect(unknown.querySelector('ul[aria-label="Intended file operations"]')).toBeNull();
      expect(unknown.textContent).not.toContain("unresolved target");
      const read = await render(<ToolCallBubble event={singleEvent(codexChain("readonly-file", "Get-Content source.txt -Raw"))} expanded={false} onToggle={() => {}} />);
      expect(read.querySelector('ul[aria-label="Intended file operations"]')).toBeNull();

      const stream = await render(<ToolCallBubble event={{ ...main, stream: { flowId: "file-operation-stream", active: false, members: [main], entries: [{ kind: "action", event: main }] } }} expanded onToggle={() => {}} />);
      expect(stream.querySelectorAll('ul[aria-label="Intended file operations"]')).toHaveLength(1);
      await click(stream.querySelector('[data-testid="chat-tool-stream-action"]'));
      expect(stream.querySelectorAll('ul[aria-label="Intended file operations"]')).toHaveLength(1);

      const diff = { path: "C:/work/preview.ts", kind: "modify" as const, additions: 1, deletions: 1, hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-old", "+new"] }] };
      for (const kind of ["command", "edit"] as const) {
        const exact = await render(<ToolCallBubble event={{ ...main, id: `exact-${kind}`, kind, fileDiffs: [diff] }} expanded={false} onToggle={() => {}} />);
        expect(exact.querySelectorAll('[data-testid="file-diff-card"]')).toHaveLength(1);
        expect(exact.querySelector('ul[aria-label="Intended file operations"]')).toBeNull();
        expect(exact.querySelector('[data-testid="file-diff-hunk"]')).toBeNull();
        const toggle = exact.querySelector('button[aria-label^="Show diff for"]');
        expect(toggle?.getAttribute("aria-expanded")).toBe("false");
        await click(toggle);
        expect(toggle?.getAttribute("aria-expanded")).toBe("true");
        expect(exact.querySelectorAll('[data-testid="file-diff-hunk"]')).toHaveLength(1);
      }
    } finally { lexer.mockRestore(); }
  });
});
