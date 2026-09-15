import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import { reduceToolEvents, type FileDiff, type NormalizedChatMessage, type TimelineItem, type ToolEvent } from "@codeterm/chat-core";
import { ToolCallBubble } from "../../../packages/chat-ui/src/ToolCallBubble";
import { MessageBubble } from "../../../packages/chat-ui/src/MessageBubble";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CLAUDE_SESSION = "claude-session";
const CODEX_SESSION = "codex-session";

function claudeCall(id: string, command: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  const description = patch.toolDescription;
  return {
    id, type: "tool_call", content: command, provider: "claude", sourceSessionId: CLAUDE_SESSION,
    toolName: "Bash", toolId: id, toolKind: "command", toolSummary: command, toolDetail: command, toolArgs: command,
    toolInput: { command, ...(description ? { description } : {}) }, ...patch,
  };
}

function result(toolId: string, output: string, sourceSessionId: string): NormalizedChatMessage {
  return { id: `${toolId}-result`, type: "tool_result", content: "", sourceSessionId, toolId, toolResult: output, toolError: false };
}

function codexCall(id: string, title: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  return {
    id, type: "tool_call", content: title, provider: "codex", sourceSessionId: CODEX_SESSION, toolName: "exec", toolId: id,
    toolKind: "generic", toolSummary: title, toolDetail: title, toolTitle: title, toolInput: { raw: `script ${id}` }, ...patch,
  };
}

function boundary(state: "start" | "end", turnId: string): NormalizedChatMessage {
  return { id: `flow-${state}-${turnId}`, type: "tool_flow", content: state, toolFlowId: turnId, provider: "codex", sourceSessionId: CODEX_SESSION };
}

function Row({ item }: { item: TimelineItem }) {
  const [expanded, setExpanded] = useState(false);
  if (item.kind === "message") return <p data-testid="timeline-message">{item.message.content}</p>;
  return <ToolCallBubble event={item.event} expanded={expanded} onToggle={setExpanded} />;
}

function Timeline({ messages }: { messages: NormalizedChatMessage[] }) {
  return <>{reduceToolEvents(messages).map((item) => (
    <Row key={item.kind === "message" ? item.message.id : item.event.message.id} item={item} />
  ))}</>;
}

let root: Root | null = null;
let host: HTMLElement | null = null;

afterEach(async () => {
  const mounted = root;
  if (mounted) await act(async () => mounted.unmount());
  host?.remove();
  root = null;
  host = null;
});

async function render(messages: NormalizedChatMessage[]): Promise<HTMLElement> {
  if (!host) {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  }
  const mounted = root as Root;
  await act(async () => mounted.render(<Timeline messages={messages} />));
  return host;
}

async function click(element: Element | null | undefined) {
  expect(element).toBeTruthy();
  await act(async () => (element as HTMLElement).click());
}

function streams(scope: ParentNode): HTMLElement[] {
  return [...scope.querySelectorAll<HTMLElement>('[data-testid="chat-tool-stream"]')];
}

function actionLabels(stream: HTMLElement): string[] {
  return [...stream.querySelectorAll('[data-testid="chat-tool-stream-member"]')]
    .map((member) => (member.querySelector('[data-testid="chat-tool-stream-action"]') ?? member).textContent?.trim() ?? "");
}

function streamEvent(messages: NormalizedChatMessage[], index = 0): ToolEvent {
  const item = reduceToolEvents(messages).filter((entry) => entry.kind === "tool" && entry.event.stream)[index];
  if (item?.kind !== "tool") throw new Error("expected a stream");
  return item.event;
}

const lintOutput = "lint ok\nno warnings";
const linked = [
  claudeCall("tu-1", "npm test -- --run", { toolDescription: "Run unit tests", toolFlowId: "msg_parallel" }),
  claudeCall("tu-2", "npm run lint", { toolDescription: "Lint sources", toolFlowId: "msg_parallel" }),
  claudeCall("tu-3", "npx tsc --noEmit", { toolDescription: "Type-check", toolFlowId: "msg_parallel" }),
];
const linkedResults = [
  result("tu-1", "tests passed", CLAUDE_SESSION),
  result("tu-2", lintOutput, CLAUDE_SESSION),
  result("tu-3", "no type errors", CLAUDE_SESSION),
];

describe("Claude tool activity", () => {
  it("renders linked described calls as one compact stream whose title rolls as descriptions arrive", async () => {
    const view = await render([linked[0]]);
    expect(streams(view)).toHaveLength(0);

    await render(linked.slice(0, 2));
    const [stream] = streams(view);
    expect(streams(view)).toHaveLength(1);
    expect(view.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(0);
    expect(stream.querySelector('[data-testid="chat-tool-stream-title"]')?.textContent).toBe("Lint sources");
    expect(stream.dataset.flowActive).toBe("true");
    expect(stream.style.maxHeight).toBe("200px");
    expect(stream.querySelector('[data-testid="chat-tool-stream-mini"]')?.textContent).toContain("Run unit tests");

    await render(linked);
    expect(streams(view)[0].querySelector('[data-testid="chat-tool-stream-title"]')?.textContent).toBe("Type-check");

    await render([...linked, ...linkedResults]);
    const settled = streams(view)[0];
    expect(settled.dataset.flowActive).toBe("false");
    expect(settled.querySelector('[data-testid="chat-tool-stream-mini"]')).toBeNull();
    expect(settled.style.maxHeight).toBe("");
  });

  it("expands to every action and opens the exact command, input, and output of the selected one", async () => {
    const messages = [...linked, ...linkedResults];
    const view = await render(messages);
    const [stream] = streams(view);
    const toggle = stream.querySelector<HTMLButtonElement>('button[aria-label="Expand 3 actions"]');
    await click(toggle);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(actionLabels(stream)).toEqual(["Run unit tests", "Lint sources", "Type-check"]);

    await click(stream.querySelector('button[aria-label="Show Lint sources"]'));
    const card = stream.querySelector<HTMLElement>('[data-testid="chat-tool-call"]');
    expect(card?.querySelector('[data-testid="toolcard-summary"]')?.textContent).toBe("Lint sources");
    expect(card?.querySelector('[data-testid="toolcard-result"]')?.textContent).toContain("no warnings");
    await click([...(card?.querySelectorAll("button") ?? [])].find((button) => button.textContent?.includes("Show raw command")));
    expect(card?.querySelector('[data-testid="message-raw-content"]')?.textContent).toBe("npm run lint");

    const member = streamEvent(messages).stream?.members[1];
    expect(member?.message.toolInput).toEqual({ command: "npm run lint", description: "Lint sources" });
    expect(member?.detail).toBe("npm run lint");
    expect(member?.result).toBe(lintOutput);
  });

  it("keeps described calls without a shared flow as rows that show the description instead of the command", async () => {
    const view = await render([
      claudeCall("solo-1", "git status --short", { toolDescription: "Check worktree", toolFlowId: "msg_a" }),
      claudeCall("solo-2", "git log -1", { toolDescription: "Show last commit", toolFlowId: "msg_b" }),
      claudeCall("solo-3", "pwd", { toolDescription: "Print directory" }),
    ]);
    expect(streams(view)).toHaveLength(0);
    const summaries = [...view.querySelectorAll('[data-testid="toolcard-summary"]')].map((node) => node.textContent);
    expect(summaries).toEqual(["Check worktree", "Show last commit", "Print directory"]);
    expect(view.querySelector('[data-testid="chat-tool-call"]')?.textContent).toContain("command");
  });

  it("keeps undescribed calls on today's row title and expansion even when they share a flow", async () => {
    const plain = [
      claudeCall("read-1", "src/a.ts", { toolName: "Read", toolKind: "read", toolInput: { file_path: "src/a.ts" } }),
      claudeCall("grep-1", "TODO", { toolName: "Grep", toolKind: "search", toolInput: { pattern: "TODO" } }),
    ];
    const rowHeaders = (scope: HTMLElement) => [...scope.querySelectorAll('[data-testid="chat-tool-call"]')]
      .map((card) => card.querySelector("button[aria-controls]")?.textContent);
    const baseline = rowHeaders(await render(plain));
    const view = await render(plain.map((message) => ({ ...message, toolFlowId: "msg_x", toolTitle: "Derived title" })));
    expect(streams(view)).toHaveLength(0);
    const cards = [...view.querySelectorAll<HTMLElement>('[data-testid="chat-tool-call"]')];
    expect(cards).toHaveLength(2);
    expect(rowHeaders(view)).toEqual(baseline);
    expect(view.textContent).not.toContain("Derived title");
    expect(cards[0].querySelector('[data-testid="toolcard-summary"]')?.textContent).toBe("a.ts");
    const toggle = cards[0].querySelector<HTMLButtonElement>("button[aria-controls]");
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    await click(toggle);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
  });
});

describe("Codex tool activity", () => {
  const turnMessages = [
    boundary("start", "turn-a"),
    codexCall("call-a1", "npm test", { toolKind: "command", toolDetail: "npm test -- --run" }),
    { id: "narration", type: "assistant", content: "checking output", sourceSessionId: CODEX_SESSION } as NormalizedChatMessage,
    codexCall("call-a2", "Wait for command output"),
    codexCall("call-a3", "wait_agent", { toolName: "wait_agent", toolInput: { ids: ["agent-1"], timeout_ms: 30000 } }),
    result("call-a1", "12 passed", CODEX_SESSION),
    result("call-a2", "poll output", CODEX_SESSION),
    result("call-a3", "agent finished", CODEX_SESSION),
    boundary("end", "turn-a"),
    boundary("start", "turn-b"),
    codexCall("call-b1", "Read shot.png", { toolKind: "read", toolSummary: "/repo/shot.png" }),
    codexCall("call-b2", "git status", { toolKind: "command", toolDetail: "git status" }),
  ];

  it("groups each contiguous run of a turn into a stream with honest titles and wait follow-ups as ordinary actions", async () => {
    const view = await render(turnMessages);
    const [first, second] = streams(view);
    expect(streams(view)).toHaveLength(2);
    expect(view.querySelector('[data-testid="chat-wait-batch"]')).toBeNull();
    const narration = view.querySelector('[data-testid="timeline-message"]');
    expect(narration?.textContent).toBe("checking output");
    const loose = view.querySelector<HTMLElement>('[data-testid="chat-tool-call"]');
    expect(loose?.compareDocumentPosition(narration as Node)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(narration?.compareDocumentPosition(first)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(first.querySelector('[data-testid="chat-tool-stream-title"]')?.textContent).toBe("wait_agent");
    expect(first.dataset.flowActive).toBe("false");
    expect(second.dataset.flowActive).toBe("true");

    await click(first.querySelector('button[aria-label="Expand 2 actions"]'));
    expect(actionLabels(first)).toEqual(["Wait for command output", "wait_agent"]);
    await click(first.querySelector('button[aria-label="Show Wait for command output"]'));
    expect(first.querySelector('[data-testid="chat-tool-call"]')?.textContent).toContain("poll output");

    await click(second.querySelector('button[aria-label="Expand 2 actions"]'));
    expect(actionLabels(second)).toEqual(["Read shot.png", "git status"]);
  });

  it("keeps calls from different turns apart and preserves exact per-call payloads", () => {
    const first = streamEvent(turnMessages, 0);
    const second = streamEvent(turnMessages, 1);
    expect(first.stream?.flowId).toBe("turn-a");
    expect(second.stream?.flowId).toBe("turn-b");
    expect(first.stream?.members.map((member) => member.id)).toEqual(["call-a2", "call-a3"]);
    const loose = reduceToolEvents(turnMessages).find((item) => item.kind === "tool" && item.event.id === "call-a1");
    expect(loose?.kind === "tool" && loose.event.detail).toBe("npm test -- --run");
    expect(loose?.kind === "tool" && loose.event.result).toBe("12 passed");
    expect(first.stream?.members[1].message.toolInput).toEqual({ ids: ["agent-1"], timeout_ms: 30000 });
  });

  it("shows a static no-result state for a turn the source moved past and keeps the spinner for the latest open turn", async () => {
    const view = await render([
      boundary("start", "turn-old"),
      codexCall("old-1", "npm test", { toolKind: "command", toolDetail: "npm test" }),
      result("old-1", "12 passed", CODEX_SESSION),
      codexCall("old-2", "npm run deploy", { toolKind: "command", toolDetail: "npm run deploy" }),
      boundary("start", "turn-new"),
      codexCall("new-1", "git status", { toolKind: "command", toolDetail: "git status" }),
      codexCall("new-2", "Wait for command output"),
    ]);
    const [stale, open] = streams(view);
    expect(streams(view)).toHaveLength(2);
    expect(stale.dataset.toolStatus).toBe("unresolved");
    expect(stale.dataset.flowActive).toBe("false");
    expect(stale.querySelector('[data-testid="chat-tool-stream-spinner"]')).toBeNull();
    expect(stale.querySelector('[data-testid="chat-tool-stream-status"]')?.getAttribute("aria-label")).toBe("no result");

    await click(stale.querySelector('button[aria-label="Expand 2 actions"]'));
    await click(stale.querySelector('button[aria-label="Show npm run deploy"]'));
    const card = stale.querySelector<HTMLElement>('[data-testid="chat-tool-call"]');
    expect(card?.dataset.toolStatus).toBe("unresolved");
    expect(card?.querySelector('[data-testid="toolcard-no-result"]')?.textContent).toBe("no result");
    expect(card?.querySelector('[data-testid="toolcard-status"]')?.getAttribute("aria-label")).toBe("no result");
    expect(card?.querySelector('[class*="spin"], [data-testid="toolcard-spinner"]')).toBeNull();

    expect(open.dataset.toolStatus).toBe("pending");
    expect(open.dataset.flowActive).toBe("true");
    expect(open.querySelector('[data-testid="chat-tool-stream-spinner"]')).not.toBeNull();
  });

  it("keeps an open turn of finished real-shape calls running and folds command activity echoes into their call", async () => {
    const view = await render([
      boundary("start", "turn-live"),
      codexCall("call-list", "npm test", { toolKind: "command", toolDetail: "npm test" }),
      result("call-list", "Script completed\nWall time 1.2 seconds\nOutput:\n  12 passed\n", CODEX_SESSION),
      codexCall("exec-echo", "npm test", { toolName: "command_execution", toolKind: "command", toolFlowId: "turn-live", toolEcho: true, toolError: false }),
      result("exec-echo", "12 passed", CODEX_SESSION),
      codexCall("call-string", "git status", { toolName: "exec_command", toolKind: "command", toolDetail: "git status" }),
      result("call-string", "clean", CODEX_SESSION),
    ]);
    const [stream] = streams(view);
    expect(streams(view)).toHaveLength(1);
    expect(stream.dataset.toolStatus).toBe("pending");
    expect(stream.querySelector('[data-testid="chat-tool-stream-status"] [data-testid="chat-tool-stream-spinner"]')).not.toBeNull();
    await click(stream.querySelector('button[aria-label="Expand 2 actions"]'));
    expect(stream.querySelectorAll('[data-testid="chat-tool-stream-member"] [data-testid="chat-tool-stream-spinner"]')).toHaveLength(0);
    expect(actionLabels(stream)).toEqual(["npm test", "git status"]);
    expect(view.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(0);
  });

  it("leaves calls without a proven turn as individual rows", async () => {
    const view = await render([codexCall("loose-1", "npm test"), codexCall("loose-2", "git status")]);
    expect(streams(view)).toHaveLength(0);
    expect(view.querySelectorAll('[data-testid="chat-tool-call"]')).toHaveLength(2);
  });
});

const LIVE_TITLES = [
  "Get-Content scripts/run-ts-unit-tests.mjs", "rg --files src", "git status", "Read package.json", "npm run lint",
  "rg -n tool_flow", "Get-Content vitest.config.ts", "git diff --stat", "npm test",
];

function liveTurn(prefix: string, flowId: string): NormalizedChatMessage[] {
  const rows: NormalizedChatMessage[] = [
    { id: `${prefix}flow-start`, type: "tool_flow", content: "start", toolFlowId: flowId, sourceSessionId: CODEX_SESSION, seq: 0 },
  ];
  LIVE_TITLES.forEach((title, index) => {
    const toolId = `${flowId}-call-${index}`;
    rows.push({ ...codexCall(toolId, title, { toolKind: "command", toolDetail: title, seq: index * 2 + 1 }), id: `${prefix}call-${index}` });
    rows.push({ ...result(toolId, `output ${index}`, CODEX_SESSION), id: `${prefix}result-${index}`, seq: index * 2 + 2 });
    if (index === 3) rows.push({ id: `${prefix}thought`, type: "thinking", content: "**Run the unit tests**", sourceSessionId: CODEX_SESSION, seq: 50 });
  });
  return rows;
}

function BubbleTimeline({ messages }: { messages: NormalizedChatMessage[] }) {
  return <>{reduceToolEvents(messages).map((item) => {
    const message = item.kind === "message" ? item.message : item.event.message;
    return <MessageBubble key={message.id} message={message} item={item.kind === "tool" ? item : undefined} />;
  })}</>;
}

async function renderBubbles(messages: NormalizedChatMessage[]): Promise<HTMLElement> {
  if (!host) {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  }
  const mounted = root as Root;
  await act(async () => mounted.render(<BubbleTimeline messages={messages} />));
  return host;
}

describe("live Codex turn through MessageBubble", () => {
  it("heads an open turn of finished calls as running and expands to every action, surviving a row re-key", async () => {
    const view = await renderBubbles(liveTurn("delta:", "turn-live-open"));
    const [stream] = streams(view);
    expect(streams(view)).toHaveLength(1);
    expect(stream.dataset.flowActive).toBe("true");
    expect(stream.dataset.toolStatus).toBe("pending");
    expect(stream.querySelector('[data-testid="chat-tool-stream-status"]')?.getAttribute("aria-label")).toBe("pending");
    expect(stream.querySelector('[data-testid="chat-tool-stream-status"] [data-testid="chat-tool-stream-spinner"]')).not.toBeNull();

    const toggle = stream.querySelector<HTMLButtonElement>('button[aria-label="Expand 9 actions"]');
    await click(toggle);
    expect(toggle?.getAttribute("aria-expanded")).toBe("true");
    expect(stream.querySelectorAll('[data-testid="chat-tool-stream-member"]')).toHaveLength(9);
    expect(stream.querySelectorAll('[data-testid="chat-tool-stream-thought"]')).toHaveLength(1);
    expect(stream.querySelectorAll('[data-testid="chat-tool-stream-member"] [data-testid="chat-tool-stream-spinner"]')).toHaveLength(0);

    await click(stream.querySelector('button[aria-label="Show npm run lint"]'));
    expect(stream.querySelector('[data-testid="chat-tool-call"]')?.textContent).toContain("output 4");

    await renderBubbles(liveTurn("snapshot:", "turn-live-open"));
    const [rekeyed] = streams(view);
    expect(rekeyed.querySelector('button[aria-label="Collapse 9 actions"]')?.getAttribute("aria-expanded")).toBe("true");
    expect(rekeyed.querySelectorAll('[data-testid="chat-tool-stream-member"]')).toHaveLength(9);
    expect(rekeyed.querySelector('[data-testid="chat-tool-call"]')?.textContent).toContain("output 4");
  });

  it("heads the turn as done once the source closes it", async () => {
    const view = await renderBubbles([
      ...liveTurn("closed:", "turn-live-closed"),
      { id: "closed:flow-end", type: "tool_flow", content: "end", toolFlowId: "turn-live-closed", sourceSessionId: CODEX_SESSION, seq: 99 },
    ]);
    const [stream] = streams(view);
    expect(stream.dataset.flowActive).toBe("false");
    expect(stream.dataset.toolStatus).toBe("success");
    expect(stream.querySelector('[data-testid="chat-tool-stream-spinner"]')).toBeNull();
  });
});

describe("stream presentation", () => {
  it("gates motion behind reduced-motion variants and uses theme tokens without blinking dots or right borders", async () => {
    const view = await render(linked.slice(0, 2));
    const [stream] = streams(view);
    const spinner = stream.querySelector('[data-testid="chat-tool-stream-spinner"]');
    const spinnerClasses = spinner?.getAttribute("class")?.split(/\s+/) ?? [];
    const spinUtilities = spinnerClasses.filter((name) => name.endsWith("spin"));
    expect(spinUtilities.length).toBeGreaterThan(0);
    expect(spinUtilities.every((name) => name.startsWith("motion-safe:"))).toBe(true);
    expect(stream.querySelector('[data-testid="chat-tool-stream-title"]')?.className).toContain("motion-reduce:transition-none");
    expect(stream.querySelector('[class*="animate-pulse"], [class*="animate-ping"]')).toBeNull();
    expect(stream.className.split(/\s+/)).toEqual(expect.not.arrayContaining(["border", "border-r", "border-x"]));
    expect(stream.style.borderRightWidth).toBe("");
    expect(stream.getAttribute("style")).toContain("var(--ct-");
  });
});

const SPEC_CHAIN = "codeterm spec section update --help ; codeterm plan task update --help";
const PLAN_CHAIN = "codeterm plan doctor ; codeterm plan decision list ; codeterm task show ; codeterm task show";

function chainCall(id: string, title: string, command: string): NormalizedChatMessage {
  return codexCall(id, title, { toolName: "exec_command", toolKind: "command", toolDetail: command, toolSummary: command, toolInput: { cmd: command } });
}

function inboundReport(id: string): NormalizedChatMessage {
  return { id, type: "user", content: "STATUS: in_review", fromLabel: "Hopper", fromTab: "e7f14bec", inboundKind: "report", sourceSessionId: CODEX_SESSION };
}

function followsInDocument(earlier: Element | null | undefined, later: Element | null | undefined): boolean {
  return Boolean(earlier && later && earlier.compareDocumentPosition(later) & Node.DOCUMENT_POSITION_FOLLOWING);
}

describe("chained commands inside a stream", () => {
  const chainTurn = [
    boundary("start", "turn-chain"),
    chainCall("chain-2", "codeterm spec", SPEC_CHAIN),
    chainCall("chain-4", "codeterm plan", PLAN_CHAIN),
    result("chain-2", "usage", CODEX_SESSION),
    result("chain-4", "doctor: ok", CODEX_SESSION),
  ];

  function treeLeads(scope: ParentNode): string[] {
    return [...scope.querySelectorAll('[data-testid="chat-command-tree-line"] [data-testid="chat-command-tree-lead"]')].map((node) => node.textContent ?? "");
  }

  it("shows each chained member's segments as a capped compact tree while the member is collapsed", async () => {
    const view = await render(chainTurn);
    const [stream] = streams(view);
    await click(stream.querySelector('button[aria-label="Expand 2 actions"]'));
    const members = [...stream.querySelectorAll<HTMLElement>('[data-testid="chat-tool-stream-member"]')];
    expect(members).toHaveLength(2);
    expect(stream.querySelector('[data-testid="chat-command-group"]')).toBeNull();
    expect(members[0].querySelector('[data-testid="chat-tool-stream-action"]')?.textContent).toContain("codeterm spec");
    expect(treeLeads(members[0])).toEqual(["spec section update", "plan task update"]);
    expect(treeLeads(members[1])).toEqual(["plan doctor", "plan decision list", "task show"]);
    expect(members[1].querySelector('[data-testid="chat-command-tree-more"]')?.textContent).toContain("+1 more");
    expect(members[1].querySelectorAll('[data-testid="chat-command-tree-operator"]')).toHaveLength(0);
  });

  it("opens the clicked segment inside the member's group with the raw command still collapsed", async () => {
    const view = await render(chainTurn);
    const [stream] = streams(view);
    await click(stream.querySelector('button[aria-label="Expand 2 actions"]'));
    const member = stream.querySelectorAll<HTMLElement>('[data-testid="chat-tool-stream-member"]')[1];
    await click(member.querySelectorAll('[data-testid="chat-command-tree-line"] button')[1]);

    const group = member.querySelector<HTMLElement>('[data-testid="chat-command-group"]');
    expect(group).not.toBeNull();
    const lines = [...(group?.querySelectorAll<HTMLElement>('[data-testid="chat-command-tree-line"]') ?? [])];
    expect(treeLeads(group as HTMLElement)).toEqual(["plan doctor", "plan decision list", "task show", "task show"]);
    expect(group?.querySelectorAll('[data-testid="chat-command-tree-detail"]')).toHaveLength(1);
    expect(lines[1].querySelector('[data-testid="chat-command-tree-detail"]')).not.toBeNull();
    expect(group?.querySelector('[data-testid="chat-command-group-title"]')?.textContent).not.toBe(PLAN_CHAIN);
    expect(group?.querySelector('[data-testid="message-raw-content"]')).toBeNull();
    expect(group?.querySelector('[data-testid="toolcard-result"]')).toBeNull();
  });

  it("renders one member per call id when the source reports a call twice", async () => {
    const messages = [
      boundary("start", "turn-dup"),
      chainCall("dup-call", "codeterm spec", SPEC_CHAIN),
      codexCall("other-call", "git status", { toolKind: "command", toolDetail: "git status" }),
      { ...chainCall("dup-call", "codeterm spec", SPEC_CHAIN), id: "dup-call-snapshot" },
      result("dup-call", "usage", CODEX_SESSION),
    ];
    const view = await render(messages);
    const [stream] = streams(view);
    expect(streams(view)).toHaveLength(1);
    await click(stream.querySelector('button[aria-label="Expand 2 actions"]'));
    expect(actionLabels(stream).filter((label) => label.startsWith("codeterm spec"))).toHaveLength(1);
    expect(streamEvent(messages).stream?.members.map((member) => member.toolId)).toEqual(["dup-call", "other-call"]);
  });
});

describe("contiguous tool streams", () => {
  const described = (id: string, command: string) => claudeCall(id, command, { toolDescription: `Run ${id}`, toolFlowId: "msg_run" });

  it("splits tool, tool, user message, tool, tool into two streams in source order", async () => {
    const view = await render([
      described("u-1", "npm test"),
      described("u-2", "npm run lint"),
      { id: "user-next", type: "user", content: "and the docs?", seq: 3, sourceSessionId: CLAUDE_SESSION },
      described("u-3", "npm run docs"),
      described("u-4", "git status"),
    ]);
    const [first, second] = streams(view);
    expect(streams(view)).toHaveLength(2);
    const message = view.querySelector('[data-testid="timeline-message"]');
    expect(followsInDocument(first, message)).toBe(true);
    expect(followsInDocument(message, second)).toBe(true);
    await click(first.querySelector('button[aria-label="Expand 2 actions"]'));
    await click(second.querySelector('button[aria-label="Expand 2 actions"]'));
    expect(actionLabels(first)).toEqual(["Run u-1", "Run u-2"]);
    expect(actionLabels(second)).toEqual(["Run u-3", "Run u-4"]);
  });

  it("closes a running stream at an inbound report and starts the turn's later actions below it", async () => {
    const view = await render([
      boundary("start", "turn-inbound"),
      codexCall("k-1", "Patched snakeModel.ts +3", { toolKind: "command", toolDetail: "apply patch" }),
      codexCall("k-2", "npm test", { toolKind: "command", toolDetail: "npm test" }),
      inboundReport("inbound-hopper"),
      codexCall("k-3", "git status", { toolKind: "command", toolDetail: "git status" }),
      codexCall("k-4", "git diff", { toolKind: "command", toolDetail: "git diff" }),
    ]);
    const [first, second] = streams(view);
    expect(streams(view)).toHaveLength(2);
    const inbound = [...view.querySelectorAll('[data-testid="timeline-message"]')].find((node) => node.textContent === "STATUS: in_review");
    expect(followsInDocument(first, inbound)).toBe(true);
    expect(followsInDocument(inbound, second)).toBe(true);
    expect(first.querySelector('[data-testid="chat-tool-stream-running"]')?.textContent).toBe("2 running");
    await click(second.querySelector('button[aria-label="Expand 2 actions"]'));
    expect(actionLabels(second)).toEqual(["git status", "git diff"]);
  });

  it("settles a call of the earlier stream in place when its result arrives after the stream closed", async () => {
    const before = [
      boundary("start", "turn-late"),
      codexCall("l-1", "npm test", { toolKind: "command", toolDetail: "npm test" }),
      codexCall("l-2", "npm run lint", { toolKind: "command", toolDetail: "npm run lint" }),
      inboundReport("inbound-late"),
      codexCall("l-3", "git status", { toolKind: "command", toolDetail: "git status" }),
      codexCall("l-4", "git diff", { toolKind: "command", toolDetail: "git diff" }),
    ];
    const view = await render(before);
    expect(streams(view)[0].dataset.toolStatus).toBe("pending");

    await render([...before, result("l-1", "12 passed", CODEX_SESSION), result("l-2", "lint ok", CODEX_SESSION)]);
    const [first, second] = streams(view);
    expect(streams(view)).toHaveLength(2);
    expect(first.dataset.toolStatus).toBe("success");
    expect(first.dataset.flowActive).toBe("false");
    expect(first.querySelector('[data-testid="chat-tool-stream-running"]')).toBeNull();
    expect(first.querySelector('[data-testid="chat-tool-stream-spinner"]')).toBeNull();
    expect(followsInDocument(first, view.querySelector('[data-testid="timeline-message"]'))).toBe(true);
    await click(first.querySelector('button[aria-label="Expand 2 actions"]'));
    expect(actionLabels(first)).toEqual(["npm test", "npm run lint"]);
    expect(second.dataset.toolStatus).toBe("pending");
  });
});

describe("changed lines inside a stream", () => {
  const diff: FileDiff = {
    path: "src/snakeModel.ts",
    additions: 3,
    deletions: 1,
    hunks: [{
      oldStart: 10, oldLines: 2, newStart: 10, newLines: 4,
      lines: [" const speed = 1;", "-const size = 10;", "+const size = 12;", "+const wrap = true;", "+const lives = 3;"],
    }],
  };

  it("expands a Patched member to the hunk lines of its source edit, and shows only its hunk branch while collapsed", async () => {
    const view = await render([
      boundary("start", "turn-patch"),
      codexCall("p-1", "Patched snakeModel.ts +3", { toolKind: "edit", toolSummary: diff.path, fileDiffs: [diff], toolError: false }),
      codexCall("p-2", "npm test", { toolKind: "command", toolDetail: "npm test", toolError: false }),
      { id: "p-end", type: "tool_flow", content: "end", toolFlowId: "turn-patch", sourceSessionId: CODEX_SESSION },
    ]);
    const [stream] = streams(view);
    await click(stream.querySelector('button[aria-label="Expand 2 actions"]'));
    const member = stream.querySelector<HTMLElement>('[data-testid="chat-tool-stream-member"]');
    expect(member?.querySelectorAll('[data-testid="chat-tool-stream-action"]')).toHaveLength(1);
    expect([...(member?.querySelectorAll<HTMLElement>('[data-testid="chat-command-tree-line"]') ?? [])].map((line) => line.dataset.branchKind)).toEqual(["hunk"]);
    expect(member?.querySelector('[data-testid="file-diff-line"]')).toBeNull();

    await click(stream.querySelector('button[aria-label="Show Patched snakeModel.ts +3"]'));
    const lines = [...(member?.querySelectorAll<HTMLElement>('[data-testid="file-diff-line"]') ?? [])];
    expect(lines.map((line) => line.textContent)).toEqual(diff.hunks[0].lines);
    expect(lines.map((line) => line.dataset.tone)).toEqual(["context", "del", "add", "add", "add"]);
  });
});

function hunkOf(oldStart: number, newStart: number, lines: string[]): FileDiff["hunks"][number] {
  return {
    oldStart, newStart, lines,
    oldLines: lines.filter((line) => !line.startsWith("+")).length,
    newLines: lines.filter((line) => !line.startsWith("-")).length,
  };
}

function diffOf(path: string, hunks: FileDiff["hunks"]): FileDiff {
  const lines = hunks.flatMap((hunk) => hunk.lines);
  return { path, hunks, additions: lines.filter((line) => line.startsWith("+")).length, deletions: lines.filter((line) => line.startsWith("-")).length };
}

function branchLines(scope: Element): HTMLElement[] {
  return [...scope.querySelectorAll<HTMLElement>('[data-testid="chat-command-tree-line"]')];
}

function branchText(line: HTMLElement, part: "lead" | "object" | "marker"): string | undefined {
  return line.querySelector(`[data-testid="chat-command-tree-${part}"]`)?.textContent ?? undefined;
}

function streamMembers(stream: HTMLElement): HTMLElement[] {
  return [...stream.querySelectorAll<HTMLElement>('[data-testid="chat-tool-stream-member"]')];
}

describe("inline member branches", () => {
  const POWERSHELL = "$response = Invoke-WebRequest -Uri https://example.test/api -UseBasicParsing";
  const POSIX = "head=$(git rev-parse HEAD)";

  it("labels PowerShell and posix assignments by the command they run in members and the stream header, with a quiet → variable branch", async () => {
    const view = await render([
      boundary("start", "turn-assign"),
      codexCall("as-git", "git status", { toolKind: "command", toolDetail: "git status", toolError: false }),
      codexCall("as-ps", "$response =", { toolName: "exec_command", toolKind: "command", toolDetail: POWERSHELL, toolSummary: POWERSHELL, toolInput: { cmd: POWERSHELL }, toolError: false }),
      codexCall("as-sh", "head=$(git", { toolName: "exec_command", toolKind: "command", toolDetail: POSIX, toolSummary: POSIX, toolInput: { cmd: POSIX }, toolError: false }),
    ]);
    const [stream] = streams(view);
    expect(stream.querySelector('[data-testid="chat-tool-stream-title"]')?.textContent).toBe("git rev-parse");
    await click(stream.querySelector('button[aria-label="Expand 3 actions"]'));
    expect(actionLabels(stream)).toEqual(["git status", "Invoke-WebRequest", "git rev-parse"]);
    const [plain, powershell, posix] = streamMembers(stream);
    expect(branchLines(plain)).toHaveLength(0);
    expect(branchLines(powershell).map((line) => [line.dataset.branchKind, branchText(line, "lead")])).toEqual([["assignment", "→ $response"]]);
    expect(branchLines(posix).map((line) => [line.dataset.branchKind, branchText(line, "lead")])).toEqual([["assignment", "→ head"]]);
  });

  it("shows one dim branch per hunk and opens only the clicked hunk's exact lines", async () => {
    const first = hunkOf(10, 10, [" const speed = 1;", "-const size = 10;", "+const size = 12;"]);
    const second = hunkOf(40, 40, [" export function tick() {", "+  wrap();", " }"]);
    const diff = diffOf("src/snakeModel.ts", [first, second]);
    const view = await render([
      boundary("start", "turn-hunks"),
      codexCall("h-1", "Patched snakeModel.ts +2", { toolKind: "edit", toolSummary: diff.path, fileDiffs: [diff], toolError: false }),
      codexCall("h-2", "npm test", { toolKind: "command", toolDetail: "npm test", toolError: false }),
    ]);
    const [stream] = streams(view);
    await click(stream.querySelector('button[aria-label="Expand 2 actions"]'));
    const [member] = streamMembers(stream);
    expect(branchLines(member).map((line) => line.dataset.branchKind)).toEqual(["hunk", "hunk"]);
    expect(branchLines(member).map((line) => branchText(line, "lead"))).toEqual(["@@ -10 +10", "@@ -40 +40"]);
    expect(branchLines(member).map((line) => branchText(line, "object"))).toEqual(["-const size = 10;", "+  wrap();"]);
    expect(member.querySelector('[data-testid="file-diff-line"]')).toBeNull();

    await click(branchLines(member)[1].querySelector("button"));
    expect(member.querySelectorAll('[data-testid="chat-command-tree-detail"]')).toHaveLength(1);
    expect(branchLines(member)[0].querySelector('[data-testid="chat-command-tree-detail"]')).toBeNull();
    expect([...branchLines(member)[1].querySelectorAll('[data-testid="file-diff-line"]')].map((line) => line.textContent)).toEqual(second.lines);
    expect(member.querySelector('[data-testid="file-diff-card"]')).toBeNull();
  });

  it("shows the first meaningful output line, a quiet exit marker only for a non-zero exit, and nothing for an empty member", async () => {
    const view = await render([
      boundary("start", "turn-output"),
      codexCall("o-fail", "npm run lint", { toolKind: "command", toolDetail: "npm run lint" }),
      codexCall("o-ok", "git status", { toolKind: "command", toolDetail: "git status" }),
      codexCall("o-empty", "git fetch", { toolKind: "command", toolDetail: "git fetch", toolError: false }),
      { id: "o-fail-result", type: "tool_result", content: "", sourceSessionId: CODEX_SESSION, toolId: "o-fail", toolResult: "Exit code: 2\nWall time: 0.4 seconds\nOutput:\n\nsrc/a.ts:3 lint failed\nsecond line", toolExitCode: 2 },
      { id: "o-ok-result", type: "tool_result", content: "", sourceSessionId: CODEX_SESSION, toolId: "o-ok", toolResult: "Exit code: 0\nOutput:\nnothing to commit", toolExitCode: 0 },
    ]);
    const [stream] = streams(view);
    await click(stream.querySelector('button[aria-label="Expand 3 actions"]'));
    const [failed, ok, empty] = streamMembers(stream);
    expect(branchLines(failed).map((line) => [line.dataset.branchKind, branchText(line, "lead"), branchText(line, "marker")])).toEqual([["output", "src/a.ts:3 lint failed", "exit 2"]]);
    expect(branchLines(ok).map((line) => [line.dataset.branchKind, branchText(line, "lead"), branchText(line, "marker")])).toEqual([["output", "nothing to commit", undefined]]);
    expect(empty.querySelector('[data-testid="chat-command-tree"]')).toBeNull();

    await click(branchLines(failed)[0].querySelector("button"));
    expect(stream.querySelectorAll('[data-testid="chat-command-tree-detail"]')).toHaveLength(1);
    expect(failed.querySelector('[data-testid="chat-branch-output"]')?.textContent).toContain("second line");
  });

  it("caps a member's branches at three with +N more", async () => {
    const diff = diffOf("src/many.ts", [1, 20, 40, 60, 80].map((start) => hunkOf(start, start, [`-old ${start}`, `+new ${start}`])));
    const view = await render([
      boundary("start", "turn-cap"),
      codexCall("c-1", "Patched many.ts +5", { toolKind: "edit", toolSummary: diff.path, fileDiffs: [diff], toolError: false }),
      codexCall("c-2", "npm test", { toolKind: "command", toolDetail: "npm test", toolError: false }),
    ]);
    const [stream] = streams(view);
    await click(stream.querySelector('button[aria-label="Expand 2 actions"]'));
    const [member] = streamMembers(stream);
    expect(branchLines(member)).toHaveLength(3);
    const more = member.querySelector('[data-testid="chat-command-tree-more"]');
    expect(more?.textContent).toContain("+2 more");
    await click(more);
    expect(branchLines(member).map((line) => branchText(line, "lead"))).toEqual(["@@ -1 +1", "@@ -20 +20", "@@ -40 +40", "@@ -60 +60", "@@ -80 +80"]);
    expect(member.querySelector('[data-testid="chat-command-tree-more"]')).toBeNull();
  });

  it("reads JSON output as its first scalar entry and renders no branch for bare structure", async () => {
    const jsonResult = (toolId: string, output: string): NormalizedChatMessage => ({
      id: `${toolId}-result`, type: "tool_result", content: "", sourceSessionId: CODEX_SESSION, toolId, toolResult: output, toolExitCode: 0,
    });
    const view = await render([
      boundary("start", "turn-json"),
      codexCall("j-send", "codeterm send", { toolKind: "command", toolDetail: "codeterm send --tab e3fa0754 --from-pad report.md" }),
      codexCall("j-empty", "git status", { toolKind: "command", toolDetail: "git status" }),
      codexCall("j-wrapped", "codeterm plan show", { toolKind: "command", toolDetail: "codeterm plan show --json" }),
      jsonResult("j-send", "{\n  \"ok\": true,\n  \"id\": \"x\"\n}"),
      jsonResult("j-empty", "{\n}\n[\n]"),
      jsonResult("j-wrapped", "Exit code: 0\nOutput:\n{\n  \"plan\": {\n    \"title\": \"Night work\"\n  }\n}"),
    ]);
    const [stream] = streams(view);
    await click(stream.querySelector('button[aria-label="Expand 3 actions"]'));
    const [send, empty, wrapped] = streamMembers(stream);
    expect(branchLines(send).map((line) => [line.dataset.branchKind, branchText(line, "lead")])).toEqual([["output", "ok true"]]);
    expect(stream.textContent).not.toContain("└{");
    expect(empty.querySelector('[data-testid="chat-command-tree"]')).toBeNull();
    expect(branchLines(wrapped).map((line) => branchText(line, "lead"))).toEqual(["title Night work"]);
  });
});
