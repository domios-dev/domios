import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { reduceToolEvents, type FileDiff, type NormalizedChatMessage, type TimelineItem, type ToolEvent } from "@codeterm/chat-core";
import { DEFAULT_THEME, themeToCSS } from "../../../../src/lib/themeCss";
import { ToolCallBubble } from "../ToolCallBubble";
import { FileDiffCards } from "./FileDiffCards";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type Provider = "claude" | "codex";
interface RichBlock { kind: string; data: Record<string, unknown> }
interface RichRow { role: string; blocks: RichBlock[] }
interface SessionParser { parseSessionDelta(chunk: string): { messages: RichRow[] } }
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type RawRecord = Record<string, any>;

const parsers = {} as Record<Provider, SessionParser>;
const roots: Root[] = [];

function fixture(provider: Provider): string {
  // Vitest runs from the repo root; import.meta.url does not map to the source path here.
  return readFileSync(resolve(process.cwd(), `plugins/provider-${provider}/tests/fixtures/file-diffs.jsonl`), "utf8");
}

function fixtureRecords(provider: Provider): RawRecord[] {
  return fixture(provider).split("\n").filter(Boolean).flatMap((line) => {
    try { return [JSON.parse(line) as RawRecord]; } catch { return []; }
  });
}

// Mirrors the host's rich-row flattening for the fields these cards read; tab_chat tests pin the Rust side.
function chatMessages(rows: RichRow[], session: string): NormalizedChatMessage[] {
  return rows.flatMap((row, rowIndex) => row.blocks.flatMap((block, blockIndex): NormalizedChatMessage[] => {
    const data = block.data;
    const id = `${session}-${rowIndex}-${blockIndex}`;
    const fileDiffs = data.file_diffs === undefined ? {} : { fileDiffs: data.file_diffs as FileDiff[] };
    if (block.kind === "text") {
      return [{ id, type: row.role === "user" ? "user" : "assistant", content: String(data.text ?? ""), sourceSessionId: session }];
    }
    if (block.kind === "tool_use") {
      return [{
        id, type: "tool_call", content: String(data.tool_summary ?? ""), sourceSessionId: session,
        toolName: String(data.name ?? ""), toolId: data.id as string | undefined, toolInput: data.input as Record<string, unknown> | undefined,
        toolKind: data.tool_kind as ToolEvent["kind"] | undefined, toolSummary: data.tool_summary as string | undefined,
        toolDetail: data.tool_detail as string | undefined, toolArgs: data.tool_detail as string | undefined,
        toolEdits: data.tool_edits as NormalizedChatMessage["toolEdits"], toolDescription: data.tool_description as string | undefined,
        toolTitle: data.tool_title as string | undefined, toolFlowId: data.tool_flow_id as string | undefined,
        toolError: data.tool_status === "success" ? false : data.tool_status === "error" ? true : undefined,
        ...fileDiffs,
      }];
    }
    if (block.kind === "tool_result") {
      return [{
        id, type: "tool_result", content: "", sourceSessionId: session, toolId: data.tool_use_id as string,
        toolResult: typeof data.content === "string" ? data.content : "", toolError: data.is_error as boolean | undefined,
        ...fileDiffs,
      }];
    }
    return [];
  }));
}

function timeline(provider: Provider): TimelineItem[] {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  try {
    return reduceToolEvents(chatMessages(parsers[provider].parseSessionDelta(fixture(provider)).messages, `${provider}-session`));
  } finally {
    warn.mockRestore();
  }
}

function toolEvents(items: TimelineItem[]): ToolEvent[] {
  return items.flatMap((item) => item.kind === "tool" ? [item.event] : []);
}

function diffsOf(provider: Provider): FileDiff[] {
  return toolEvents(timeline(provider)).flatMap((event) => event.fileDiffs ?? []);
}

function sharedDiffs(provider: Provider): FileDiff[] {
  return diffsOf(provider).filter((diff) => diff.path === "src/shared.ts" || diff.path === "src/created.ts");
}

function eventFor(provider: Provider, toolId: string): ToolEvent {
  const event = toolEvents(timeline(provider)).find((candidate) => candidate.toolId === toolId);
  if (!event) throw new Error(`missing ${provider} event ${toolId}`);
  return event;
}

function Timeline({ items }: { items: TimelineItem[] }) {
  return (
    <>
      {items.map((item) => item.kind === "message"
        ? <p key={item.message.id} data-testid="timeline-message">{item.message.content}</p>
        : <ToolCallBubble key={item.event.id} event={item.event} expanded={false} onToggle={() => undefined} />)}
    </>
  );
}

async function mount(node: ReactElement): Promise<HTMLElement> {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  await act(async () => root.render(node));
  return host;
}

function all(host: ParentNode, testId: string): HTMLElement[] {
  return [...host.querySelectorAll<HTMLElement>(`[data-testid="${testId}"]`)];
}

async function expandAll(host: HTMLElement) {
  for (const toggle of all(host, "file-diff-toggle")) await act(async () => toggle.click());
}

beforeAll(async () => {
  (globalThis as { host?: unknown }).host ??= { manifestJson: () => "null", log: () => undefined };
  const [claude, codex] = await Promise.all([
    import("../../../../plugins/provider-claude/src/plugin"),
    import("../../../../plugins/provider-codex/src/plugin"),
  ]);
  parsers.claude = claude.default as unknown as SessionParser;
  parsers.codex = codex.default as unknown as SessionParser;
});

afterEach(() => {
  for (const root of roots.splice(0)) act(() => root.unmount());
  document.body.innerHTML = "";
});

describe("file diff cards", () => {
  it("normalizes Claude structuredPatch and Codex FileChange into one per-file capability", () => {
    const heads = (provider: Provider) => diffsOf(provider).map((diff) => [diff.path, diff.movePath, diff.additions, diff.deletions]);
    expect(heads("claude")).toEqual([
      ["src/shared.ts", undefined, 1, 1],
      ["src/created.ts", undefined, 2, 0],
    ]);
    expect(heads("codex")).toEqual([
      ["src/shared.ts", undefined, 1, 1],
      ["src/created.ts", undefined, 2, 0],
      ["src/removed.ts", undefined, 0, 1],
      ["src/legacy.ts", "src/renamed.ts", 1, 1],
    ]);
    expect(sharedDiffs("claude")).toEqual(sharedDiffs("codex"));
    const claudeLines = fixtureRecords("claude")[1].toolUseResult.structuredPatch[0].lines;
    const codexLines = (fixtureRecords("codex")[0].payload.item.changes["src/shared.ts"].unified_diff as string).split("\n").slice(1, -1);
    expect(sharedDiffs("claude")[0].hunks[0].lines).toEqual(claudeLines);
    expect(sharedDiffs("codex")[0].hunks[0].lines).toEqual(codexLines);
  });

  it("renders one collapsed card per changed file headed by path +N −M", async () => {
    const codexHost = await mount(<Timeline items={timeline("codex")} />);
    expect(all(codexHost, "file-diff-header").map((node) => node.textContent)).toEqual([
      "src/shared.ts +1 −1",
      "src/created.ts +2 −0",
      "src/removed.ts +0 −1",
      "src/legacy.ts → src/renamed.ts +1 −1",
    ]);
    expect(all(all(codexHost, "file-diff-cards")[0], "file-diff-card")).toHaveLength(2);
    expect(all(codexHost, "file-diff-toggle").every((toggle) => toggle.getAttribute("aria-expanded") === "false")).toBe(true);
    expect(all(codexHost, "file-diff-line")).toHaveLength(0);

    const claudeHost = await mount(<Timeline items={timeline("claude")} />);
    expect(all(claudeHost, "file-diff-header").map((node) => node.textContent)).toEqual(["src/shared.ts +1 −1", "src/created.ts +2 −0"]);
    expect(all(claudeHost, "file-diff-cards")).toHaveLength(2);
  });

  it("discloses the exact supplied hunk detail from a keyboard-operable toggle", async () => {
    const host = await mount(<Timeline items={timeline("claude")} />);
    const toggle = all(host, "file-diff-toggle")[0] as HTMLButtonElement;
    expect(toggle.tagName).toBe("BUTTON");
    expect(toggle.getAttribute("type")).toBe("button");
    expect(toggle.hasAttribute("tabindex")).toBe(false);
    const region = document.getElementById(toggle.getAttribute("aria-controls") ?? "");
    expect(region?.getAttribute("role")).toBe("region");
    expect(region?.hidden).toBe(true);

    toggle.focus();
    expect(document.activeElement).toBe(toggle);
    await act(async () => toggle.click());
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(region?.hidden).toBe(false);
    expect(all(region as HTMLElement, "file-diff-hunk-header").map((node) => node.textContent)).toEqual(["@@ -1,3 +1,3 @@"]);
    expect(all(region as HTMLElement, "file-diff-line").map((node) => node.textContent))
      .toEqual(fixtureRecords("claude")[1].toolUseResult.structuredPatch[0].lines);
    expect(all(region as HTMLElement, "file-diff-line").map((node) => node.dataset.tone)).toEqual(["context", "del", "add", "context"]);

    await act(async () => toggle.click());
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(region?.hidden).toBe(true);
  });

  it("renders equivalent Claude and Codex diffs identically", async () => {
    const claudeHost = await mount(<FileDiffCards diffs={sharedDiffs("claude")} />);
    const codexHost = await mount(<FileDiffCards diffs={sharedDiffs("codex")} />);
    await expandAll(claudeHost);
    await expandAll(codexHost);
    const markup = (host: HTMLElement) => host.innerHTML.replace(/\s(?:id|aria-controls)="[^"]*"/g, "");
    expect(all(claudeHost, "file-diff-line").length).toBe(6);
    expect(markup(claudeHost)).toBe(markup(codexHost));
  });

  it("skips malformed diff data while the ordinary action and other records stay visible", async () => {
    const broken = eventFor("claude", "toolu_edit_bad");
    expect(broken.kind).toBe("edit");
    expect(broken.fileDiffs).toBeUndefined();
    expect(eventFor("claude", "toolu_edit_nopath").fileDiffs).toBeUndefined();
    const claudeHost = await mount(<Timeline items={timeline("claude")} />);
    expect(all(claudeHost, "chat-tool-call")).toHaveLength(5);
    expect(all(claudeHost, "file-diff-card")).toHaveLength(2);
    expect(all(claudeHost, "timeline-message").map((node) => node.textContent)).toContain("Both files are updated.");

    const partial = eventFor("codex", "exec-file-change-bad");
    expect(partial.fileDiffs?.map((diff) => diff.path)).toEqual(["src/removed.ts"]);
    const codexHost = await mount(<Timeline items={timeline("codex")} />);
    expect(all(codexHost, "chat-tool-call")).toHaveLength(3);
    expect(all(codexHost, "file-diff-header").some((node) => node.textContent?.includes("src/broken.ts"))).toBe(false);
    expect(all(codexHost, "timeline-message").map((node) => node.textContent)).toContain("Both files are updated.");

    const valid = sharedDiffs("codex")[0];
    const miscounted: FileDiff = { ...valid, additions: valid.additions + 4 };
    const call = (fileDiffs: FileDiff[]): NormalizedChatMessage => ({
      id: "call", type: "tool_call", content: "", toolId: "call", toolKind: "edit", toolSummary: valid.path, fileDiffs,
    });
    expect(toolEvents(reduceToolEvents([call([miscounted, valid])]))[0].fileDiffs).toEqual([valid]);
    expect(toolEvents(reduceToolEvents([call([miscounted])]))[0].fileDiffs).toBeUndefined();
  });

  it("leaves rows without a diff capability and stream grouping unchanged", async () => {
    const plainHost = await mount(<ToolCallBubble event={eventFor("claude", "toolu_bash_e")} expanded={false} onToggle={() => undefined} />);
    expect(all(plainHost, "file-diff-cards")).toHaveLength(0);
    expect(all(plainHost, "chat-tool-call")).toHaveLength(1);

    const editHost = await mount(<ToolCallBubble event={eventFor("claude", "toolu_edit_a")} expanded={false} onToggle={() => undefined} />);
    expect(editHost.firstElementChild?.getAttribute("data-testid")).toBe("chat-tool-call");
    expect(editHost.querySelector('[data-testid="toolcard-edit-summary"]')?.textContent).toBe("+1 -1");

    const diff = sharedDiffs("claude")[0];
    const flowCall = (id: string, patch: Partial<NormalizedChatMessage>): NormalizedChatMessage => ({
      id, type: "tool_call", content: id, toolId: id, toolKind: "edit", toolSummary: id,
      toolDescription: `Describe ${id}`, toolFlowId: "msg-shared", sourceSessionId: "session", ...patch,
    });
    const items = reduceToolEvents([flowCall("a", { fileDiffs: [diff] }), flowCall("b", {})]);
    expect(items).toHaveLength(1);
    const head = toolEvents(items)[0];
    expect(head.stream?.members.map((member) => member.id)).toEqual(["a", "b"]);
    expect(head.fileDiffs).toBeUndefined();
    expect(head.stream?.members[0].fileDiffs).toEqual([diff]);
    const streamHost = await mount(<ToolCallBubble event={head} expanded={false} onToggle={() => undefined} />);
    expect(all(streamHost, "chat-tool-stream")).toHaveLength(1);
    expect(all(streamHost, "file-diff-card")).toHaveLength(0);
  });

  it("uses CodeTerm theme tokens with no right borders or motion", async () => {
    const host = await mount(<FileDiffCards diffs={diffsOf("codex")} />);
    await expandAll(host);
    const html = host.innerHTML;
    expect(html).not.toMatch(/border-r\b|border-right|animate-|transition/);
    const theme = themeToCSS(DEFAULT_THEME) as Record<string, unknown>;
    const tokens = [...new Set(html.match(/--ct-[a-z-]+/g) ?? [])];
    expect(tokens.length).toBeGreaterThan(0);
    expect(tokens.filter((token) => !(token in theme))).toEqual([]);
  });
});
