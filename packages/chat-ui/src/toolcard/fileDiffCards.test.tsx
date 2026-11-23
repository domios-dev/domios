import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { reduceToolEvents, type FileDiff, type NormalizedChatMessage, type TimelineItem, type ToolEvent } from "@codeterm/chat-core";
import { DEFAULT_THEME, themeToCSS } from "../../../../src/lib/themeCss";
import { installTestIo } from "../../../../plugins/provider-shared/tests/hostIo";
import { runtimeReduce } from "../../../../plugins/provider-codex/src/runtimeReduce";
import type { RuntimeContext } from "@codeterm/plugin-sdk";
import { ToolCallBubble } from "../ToolCallBubble";
import { FileDiffCards } from "./FileDiffCards";
import { ChatBlockHostProvider } from "../blocks/ChatBlockHost";

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
  return items.flatMap((item) => item.kind === "tool" ? item.event.editRun ?? [item.event] : []);
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

function diffEvent(fileDiffs: FileDiff[]): ToolEvent {
  return {
    id: "diff-event", phase: "call", status: "success", kind: "edit", summary: fileDiffs[0]?.path ?? "", fileDiffs,
    message: { id: "diff-event", type: "tool_call", content: "" },
  };
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
  (globalThis as { host?: unknown }).host = installTestIo({ manifestJson: () => "null", log: () => undefined });
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

  it("folds back-to-back file edits into one collapsed edit tree with a row per file and exact counts", async () => {
    const runs = (host: HTMLElement) => all(host, "chat-edit-run");
    const rows = (run: HTMLElement) => all(run, "chat-edit-run-file").map((row) => [
      row.querySelector("[title]")?.getAttribute("title"),
      row.querySelector('[data-testid="edit-run-counts"]')?.textContent,
    ]);
    const codexHost = await mount(<Timeline items={timeline("codex")} />);
    expect(runs(codexHost)).toHaveLength(1);
    const codexRun = runs(codexHost)[0];
    expect(all(codexRun, "chat-edit-run-title").map((node) => node.textContent)).toEqual(["File changes"]);
    expect(codexRun.getAttribute("aria-label")).toBe("File changes, +4 −3");
    expect(rows(codexRun)).toEqual([
      ["src/shared.ts", "+1 −1"],
      ["src/created.ts", "+2"],
      ["src/removed.ts", "−1"],
      ["src/renamed.ts", "+1 −1"],
    ]);
    expect(all(codexRun, "chat-edit-run-file")).toHaveLength(4);

    const claudeHost = await mount(<Timeline items={timeline("claude")} />);
    expect(runs(claudeHost)).toHaveLength(1);
    expect(runs(claudeHost)[0].getAttribute("aria-label")).toBe("File changes, +3 −1");
    expect(rows(runs(claudeHost)[0])).toEqual([["src/shared.ts", "+1 −1"], ["src/created.ts", "+2"]]);
    const legacyEdits = (provider: Provider) => toolEvents(timeline(provider)).filter((event) => event.kind === "edit" && !event.fileDiffs?.length).length;
    for (const [provider, host] of [["codex", codexHost], ["claude", claudeHost]] as const) {
      // Folded files never also render as standalone per-file cards, and every row starts collapsed.
      expect(all(host, "file-diff-card")).toHaveLength(0);
      expect(all(host, "edit-run-diff")).toHaveLength(0);
      expect(all(host, "chat-edit-run-file").every((row) => row.querySelector("button")?.getAttribute("aria-expanded") === "false")).toBe(true);
      for (const run of runs(host)) {
        expect(all(run, "toolcard-edit-file")).toHaveLength(0);
        expect(all(run, "toolcard-edit-summary")).toHaveLength(0);
      }
      // Only edits without a structured diff keep the old/new-string card, beside the tree in the timeline.
      expect(all(host, "toolcard-edit-summary")).toHaveLength(legacyEdits(provider));
      expect(all(host, "timeline-message").map((node) => node.textContent)).toContain("Both files are updated.");
    }
  });

  it("counts a live creation once when the turn snapshot echoes it, and keeps snapshot-only files", async () => {
    const runtimeFixture = (path: string) => JSON.parse(readFileSync(
      resolve(process.cwd(), "plugins/provider-codex/tests/fixtures", path), "utf8"));
    const { context } = runtimeFixture("runtime/tool-call.json") as { context: RuntimeContext };
    const item = runtimeFixture("captured-live-edit.json") as {
      id: string;
      changes: { path: string; diff: string }[];
    };
    const { path, diff } = item.changes[0];
    const snapshotPath = path.replace(/\\/g, "/");
    const creation = `--- /dev/null\n+++ b/${snapshotPath}\n@@ -0,0 +1,42 @@\n${diff.trimEnd().split("\n").map((line) => `+${line}`).join("\n")}\n`;
    const snapshotOnly = "--- /dev/null\n+++ b/src/other.txt\n@@ -0,0 +1 @@\n+another change\n";
    const { threadId } = context;
    if (!threadId) throw new Error("Expected a bound runtime fixture");
    const params = { threadId, turnId: "file-changes-turn" };
    const messages = (patch: string) => runtimeReduce(context, null, [
      { kind: "notification", method: "item/completed", params: { ...params, item } },
      { kind: "notification", method: "turn/diff/updated", params: { ...params, diff: patch } },
    ]).events.flatMap((event) => event.kind === "chatItem"
      ? [{ ...event.item, sourceSessionId: "live-edit-session" } as NormalizedChatMessage]
      : []);

    const duplicate = messages(creation);
    expect(duplicate).toHaveLength(2);
    expect(toolEvents(reduceToolEvents(duplicate)).map((event) => event.id)).toEqual([item.id]);
    expect(toolEvents(reduceToolEvents([...duplicate].reverse())).map((event) => event.id)).toEqual([item.id]);
    expect(toolEvents(reduceToolEvents(duplicate.slice(1)))[0]?.fileDiffs?.map((change) => change.path)).toEqual([snapshotPath]);
    expect(toolEvents(reduceToolEvents([duplicate[0], { ...duplicate[1], toolFlowId: "another-turn" }]))).toHaveLength(2);

    const items = reduceToolEvents(messages(creation + snapshotOnly));
    expect(items).toHaveLength(1);
    const head = items[0];
    if (head.kind !== "tool") throw new Error("Expected a file-change group");
    const host = await mount(<ChatBlockHostProvider value={{
      renderMenu: ({ items }) => <>{items.map((item) => <button key={item.label} onClick={item.onSelect}>{item.label}</button>)}</>,
    }}><ToolCallBubble event={head.event} expanded onToggle={() => undefined} /></ChatBlockHostProvider>);
    const run = host.querySelector('[aria-label="File changes, +43 −0"]');
    expect(run).not.toBeNull();
    const rows = all(host, "chat-edit-run-file");
    expect(rows.map((row) => row.querySelector("[title]")?.getAttribute("title"))).toEqual([path, "src/other.txt"]);
    expect(all(host, "message-raw-content")).toHaveLength(0);
    expect(all(host, "toolcard-result")).toHaveLength(0);
    await act(async () => rows[0].querySelector("button")?.click());
    expect(all(rows[0], "file-diff-line")).toHaveLength(42);
    expect(all(host, "message-raw-content")).toHaveLength(0);
    expect(host.textContent).not.toContain('"changes"');
    const rawToggle = [...host.querySelectorAll("button")].find((button) => button.textContent === "View raw");
    expect(rawToggle).toBeDefined();
    await act(async () => rawToggle?.click());
    expect(all(host, "message-raw-content").map((node) => node.textContent)).toContain(JSON.stringify(item, null, 2));
    expect(all(rows[0], "message-raw-content")).toHaveLength(0);
    await act(async () => rawToggle?.click());
    expect(all(host, "message-raw-content")).toHaveLength(0);
  });

  it("renders the captured workspace-relative snapshot and absolute Windows creation as one +84 row", async () => {
    const cwd = "C:\\Users\\User\\.domios-dev-w1221\\provisioned-workspaces\\veilfileqa-4851174b754b";
    const messages = JSON.parse(readFileSync(
      resolve(process.cwd(), "packages/chat-ui/src/toolcard/fixtures/relative-turn-diff.json"), "utf8",
    )) as NormalizedChatMessage[];
    const [created, snapshot] = messages;
    const host = await mount(<Timeline items={reduceToolEvents(messages, cwd)} />);
    expect(all(host, "chat-edit-run-file")).toHaveLength(0);
    const cards = all(host, "file-diff-card");
    expect(cards).toHaveLength(1);
    expect(cards[0].getAttribute("aria-label")).toBe("Created veil-file-change-proof.txt +84");
    await expandAll(host);
    expect(all(cards[0], "file-diff-line")).toHaveLength(84);

    expect(toolEvents(reduceToolEvents(messages, cwd.toUpperCase().replace(/\\/g, "/") + "/."))).toHaveLength(1);
    expect(toolEvents(reduceToolEvents(messages, cwd + "\\another-workspace"))).toHaveLength(2);
    const posix = [{ ...created, fileDiffs: created.fileDiffs?.map(diff => ({
      ...diff, path: "/Workspace/veil-file-change-proof.txt",
    })) }, snapshot];
    expect(toolEvents(reduceToolEvents(posix, "/Workspace"))).toHaveLength(1);
    expect(toolEvents(reduceToolEvents(posix, "/workspace"))).toHaveLength(2);
    const rootSnapshot = { ...snapshot, toolResult: snapshot.toolResult?.replaceAll(
      "veil-file-change-proof.txt", "Workspace/veil-file-change-proof.txt",
    ) };
    expect(toolEvents(reduceToolEvents([posix[0], rootSnapshot], "/"))).toHaveLength(1);
  });

  it("keeps create and edit calls for the same path as separate file change rows", async () => {
    const path = "C:/Users/User/AppData/Local/Temp/domios-open131-1d8cadc0/cards-created.txt";
    const created: FileDiff = { path, kind: "add", additions: 1, deletions: 0, hunks: [{ oldStart: 0, oldLines: 0, newStart: 1, newLines: 1, lines: ["+PUBLIC_OPEN131_CREATED"] }] };
    const edited: FileDiff = { path, kind: "modify", additions: 1, deletions: 1, hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-PUBLIC_OPEN131_CREATED", "+PUBLIC_OPEN131_EDITED"] }] };
    const message = (id: string, toolName: string, fileDiff: FileDiff): NormalizedChatMessage => ({
      id, type: "tool_call", content: "", sourceSessionId: "open131-session", toolId: id, toolName, toolKind: "edit",
      toolSummary: path, toolDetail: path, toolInput: { path }, toolStatus: "completed", toolError: false,
      toolDurationMs: 10, fileDiffs: [fileDiff],
    });
    const items = reduceToolEvents([message("create", "write", created), message("edit", "edit", edited)]);
    const host = await mount(<Timeline items={items} />);
    const run = all(host, "chat-edit-run")[0];
    const rows = all(run, "chat-edit-run-file");
    expect(all(host, "chat-edit-run")).toHaveLength(1);
    expect(all(host, "file-diff-cards")).toHaveLength(0);
    expect(rows).toHaveLength(2);
    expect(rows.map((row) => row.querySelector('[data-testid="edit-run-counts"]')?.textContent)).toEqual(["+1", "+1 −1"]);
    expect(rows.every((row) => row.querySelector('[role="status"]')?.getAttribute("aria-label"))).toBe(true);
    expect(all(run, "edit-run-duration").map((duration) => duration.textContent)).toEqual(["0s", "0s"]);
    for (const row of rows) {
      const duration = row.querySelector('[data-testid="edit-run-duration"]')!;
      const status = row.querySelector('[role="status"]')!;
      expect(duration.compareDocumentPosition(status) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    }
    await act(async () => rows[0].querySelector("button")?.click());
    expect(all(run, "edit-run-diff")).toHaveLength(1);
    expect(all(run, "edit-run-diff")[0].textContent).toContain("PUBLIC_OPEN131_CREATED");
  });

  it("labels standalone create rows and shows status and muted duration without a completion pill", async () => {
    const path = "C:/Users/User/AppData/Local/Temp/domios-open131-1d8cadc0/cards-created.txt";
    const host = await mount(<FileDiffCards event={{
      ...diffEvent([{ path, kind: "add", additions: 1, deletions: 0, hunks: [] }]), status: "success", durationMs: 0,
    }} />);
    expect(all(host, "file-diff-header")[0].textContent).toBe("Created domios-open131-1d8cadc0/cards-created.txt +1");
    expect(all(host, "file-diff-status")[0].getAttribute("aria-label")).toBe("success");
    expect(all(host, "file-diff-duration")[0].textContent).toBe("0s");
    expect(host.textContent).not.toContain("completed");
  });

  it("labels edited files and places the duration before the outcome", async () => {
    const path = "src/changed.ts";
    const host = await mount(<FileDiffCards event={{
      ...diffEvent([{ path, kind: "modify", additions: 1, deletions: 1, hunks: [] }]), status: "success", durationMs: 2800,
    }} />);
    const duration = all(host, "file-diff-duration")[0];
    const status = all(host, "file-diff-status")[0];
    expect(duration.compareDocumentPosition(status) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("merges an edit with a structured diff into one card per file with exact counts and reachable raw input", async () => {
    const editEvent = eventFor("claude", "toolu_edit_a");
    const diffs = editEvent.fileDiffs ?? [];
    expect(diffs.length).toBeGreaterThan(0);
    const opened: Array<[string, string]> = [];
    const host = await mount(<ChatBlockHostProvider value={{
      openFileChange: (path, view) => { opened.push([path, view]); },
      renderMenu: ({ items }) => <>{items.map((item) => <button key={item.label} onClick={item.onSelect}>{item.label}</button>)}</>,
    }}><ToolCallBubble event={editEvent} expanded onToggle={() => undefined} /></ChatBlockHostProvider>);
    expect(all(host, "file-diff-card")).toHaveLength(diffs.length);
    expect(all(host, "file-diff-header")).toHaveLength(diffs.length);
    expect(all(host, "chat-tool-call")).toHaveLength(0);
    await expandAll(host);
    expect(all(host, "toolcard-edit-file")).toHaveLength(0);
    const card = all(host, "file-diff-card")[0];
    expect(card.querySelector('[data-testid="message-raw-content"]')).toBeNull();
    expect(card.textContent).not.toContain("Raw command");
    const rawToggle = [...card.querySelectorAll("button")].find((button) => button.textContent === "View raw");
    expect(rawToggle).toBeDefined();
    await act(async () => rawToggle?.click());
    expect(card.querySelector('[data-testid="message-raw-content"]')?.textContent).toBe(JSON.stringify(editEvent.message.toolInput, null, 2));
    for (const label of ["View changes", "View file"]) {
      await act(async () => [...card.querySelectorAll("button")].find((button) => button.textContent === label)?.click());
    }
    expect(opened).toEqual([[diffs[0].path, "diff"], [diffs[0].path, "file"]]);
  });

  it("highlights hunk code by file extension after an untinted marker column", async () => {
    const jsDiff: FileDiff = {
      path: "games/snake.js", additions: 1, deletions: 1,
      hunks: [{ oldStart: 1, oldLines: 2, newStart: 1, newLines: 2, lines: [" const speed = 1;", "-let dir = \"left\";", "+let dir = \"right\";"] }],
    };
    const host = await mount(<FileDiffCards event={diffEvent([jsDiff])} />);
    await expandAll(host);
    const lines = all(host, "file-diff-line");
    expect(lines.map((node) => node.textContent)).toEqual(jsDiff.hunks[0].lines);
    const added = lines.find((node) => node.dataset.tone === "add") as HTMLElement;
    const removed = lines.find((node) => node.dataset.tone === "del") as HTMLElement;
    expect(added.querySelector(".token")).not.toBeNull();
    expect(removed.querySelector(".token")).not.toBeNull();
    const htmlHost = await mount(<FileDiffCards event={diffEvent([{ ...jsDiff, path: "games/snake.html" }])} initiallyExpanded />);
    expect(htmlHost.querySelector('[data-testid="file-diff-line"] .token')).not.toBeNull();
    const marker = added.querySelector<HTMLElement>('[data-testid="file-diff-marker"]') as HTMLElement;
    expect(marker.textContent).toBe("+");
    expect(marker.querySelector(".token")).toBeNull();
    expect(marker.closest(".token")).toBeNull();

    const plainHost = await mount(<FileDiffCards event={diffEvent([{ ...jsDiff, path: "notes/snake.unknownext" }])} />);
    await expandAll(plainHost);
    expect(all(plainHost, "file-diff-line").map((node) => node.textContent)).toEqual(jsDiff.hunks[0].lines);
    expect(plainHost.querySelector('[data-testid="file-diff-line"] .token')).toBeNull();
  });

  it("discloses the exact supplied hunk detail from a keyboard-operable file row in the edit tree", async () => {
    const host = await mount(<Timeline items={timeline("claude")} />);
    const row = all(host, "chat-edit-run-file")[0];
    const toggle = row.querySelector("button") as HTMLButtonElement;
    expect(toggle.tagName).toBe("BUTTON");
    expect(toggle.getAttribute("type")).toBe("button");
    expect(toggle.hasAttribute("tabindex")).toBe(false);
    expect(toggle.getAttribute("aria-label")).toBe("Show diff for src/shared.ts, change 1");
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(all(row, "edit-run-diff")).toHaveLength(0);

    toggle.focus();
    expect(document.activeElement).toBe(toggle);
    await act(async () => toggle.click());
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    const detail = all(row, "edit-run-diff")[0];
    expect(detail).toBeDefined();
    expect(detail.textContent).toContain("Lines 1–3");
    expect(all(detail, "file-diff-line").map((node) => node.textContent))
      .toEqual(fixtureRecords("claude")[1].toolUseResult.structuredPatch[0].lines);
    expect(all(detail, "file-diff-line").map((node) => node.dataset.tone)).toEqual(["context", "del", "add", "context"]);

    await act(async () => toggle.click());
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(all(row, "edit-run-diff")).toHaveLength(0);
  });

  it("renders equivalent Claude and Codex diffs identically", async () => {
    const claudeHost = await mount(<FileDiffCards event={diffEvent(sharedDiffs("claude"))} />);
    const codexHost = await mount(<FileDiffCards event={diffEvent(sharedDiffs("codex"))} />);
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
    const withoutDiffs = (provider: Provider) => toolEvents(timeline(provider)).filter((event) => !event.fileDiffs?.length).length;
    const claudeHost = await mount(<Timeline items={timeline("claude")} />);
    expect(all(claudeHost, "chat-tool-call")).toHaveLength(withoutDiffs("claude"));
    expect(all(claudeHost, "chat-edit-run-file")).toHaveLength(2);
    expect(all(claudeHost, "timeline-message").map((node) => node.textContent)).toContain("Both files are updated.");

    const partial = eventFor("codex", "exec-file-change-bad");
    expect(partial.fileDiffs?.map((diff) => diff.path)).toEqual(["src/removed.ts"]);
    const codexHost = await mount(<Timeline items={timeline("codex")} />);
    expect(all(codexHost, "chat-tool-call")).toHaveLength(withoutDiffs("codex"));
    const codexRows = all(codexHost, "chat-edit-run-file").map((node) => node.textContent ?? "");
    expect(codexRows.some((text) => text.includes("src/broken.ts"))).toBe(false);
    expect(codexRows.some((text) => text.includes("src/removed.ts"))).toBe(true);
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

    const legacyHost = await mount(<ToolCallBubble event={eventFor("claude", "toolu_edit_bad")} expanded onToggle={() => undefined} />);
    expect(all(legacyHost, "chat-tool-call")).toHaveLength(1);
    expect(all(legacyHost, "toolcard-edit-summary")).toHaveLength(1);
    expect(all(legacyHost, "file-diff-card")).toHaveLength(0);

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

    const openStream = await mount(<ToolCallBubble event={head} expanded onToggle={() => undefined} />);
    await act(async () => openStream.querySelector<HTMLButtonElement>('button[aria-label="Show Describe a"]')?.click());
    expect(all(openStream, "file-diff-card")).toHaveLength(1);
    expect(all(openStream, "file-diff-header")).toHaveLength(1);
    expect(all(openStream, "toolcard-edit-file")).toHaveLength(0);
  });

  it("renders a nested patch and its completed activity once with expandable hunks", async () => {
    const patch = "*** Begin Patch\n*** Update File: /repo/a.ts\n@@\n-before\n+after\n*** End Patch";
    const records = [
      { type: "response_item", payload: { type: "custom_tool_call", name: "exec", call_id: "nested-patch",
        input: `text(await tools.apply_patch(${JSON.stringify(patch)}));`,
        internal_chat_message_metadata_passthrough: { turn_id: "patch-turn" } } },
      { type: "event_msg", payload: { type: "item_completed", turn_id: "patch-turn", item: {
        type: "FileChange", id: "patch-activity", status: "completed",
        changes: { "/repo/a.ts": { type: "update", unified_diff: "@@ -1 +1 @@\n-before\n+after\n" } },
      } } },
      { type: "response_item", payload: { type: "custom_tool_call_output", call_id: "nested-patch", output: "Script completed" } },
    ];
    const rows = parsers.codex.parseSessionDelta(records.map(record => JSON.stringify(record)).join("\n")).messages;
    const events = toolEvents(reduceToolEvents(chatMessages(rows, "nested-patch-session")));
    expect(events).toHaveLength(1);
    const host = await mount(<ToolCallBubble event={events[0]} expanded={false} onToggle={() => undefined} />);
    expect(all(host, "file-diff-card")).toHaveLength(1);
    expect(all(host, "toolcard-edit-summary")).toHaveLength(0);
    await expandAll(host);
    expect(host.textContent).toContain("before");
    expect(host.textContent).toContain("after");
  });

  it("keeps tool detail and result in the menu disclosure for a multi-file edit", async () => {
    const notice = "ACP tool details were shortened to fit the display budget.";
    const first: FileDiff = {
      path: "src/first.ts", kind: "modify", additions: 1, deletions: 1,
      hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-oldA", "+newA"] }],
    };
    const second: FileDiff = {
      path: "src/second.ts", kind: "modify", additions: 1, deletions: 1,
      hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-oldB", "+newB"] }],
    };
    const multiHost = await mount(<ChatBlockHostProvider value={{
      renderMenu: ({ items }) => <>{items.map((item) => <button key={item.label} onClick={item.onSelect}>{item.label}</button>)}</>,
    }}><FileDiffCards event={{
      ...diffEvent([first, second]), status: "success", detail: notice, result: "Edited both files.",
    }} /></ChatBlockHostProvider>);
    expect(all(multiHost, "toolcard-result")).toHaveLength(0);
    const toggles = all(multiHost, "file-diff-toggle");
    expect(toggles).toHaveLength(2);
    for (const toggle of toggles) {
      expect(toggle.getAttribute("aria-expanded")).toBe("false");
      const region = multiHost.querySelector(`[id="${toggle.getAttribute("aria-controls")}"]`);
      expect(region?.getAttribute("role")).toBe("region");
      expect(region?.hasAttribute("hidden")).toBe(true);
    }
    await expandAll(multiHost);
    const cards = all(multiHost, "file-diff-card");
    expect(all(multiHost, "toolcard-result")).toHaveLength(0);
    expect(all(multiHost, "message-raw-content")).toHaveLength(0);
    expect(multiHost.textContent).not.toContain(notice);
    const rawToggle = [...cards[0].querySelectorAll("button")].find((button) => button.textContent === "View raw");
    expect(rawToggle).toBeDefined();
    await act(async () => rawToggle?.click());
    expect(all(cards[0], "message-raw-content").map((node) => node.textContent)).toEqual([notice, "Edited both files."]);
    expect(all(cards[1], "message-raw-content")).toHaveLength(0);
  });

  it("uses CodeTerm theme tokens with no right borders or motion", async () => {
    const host = await mount(<FileDiffCards event={diffEvent(diffsOf("codex"))} />);
    await expandAll(host);
    // The shared raw-input disclosure keeps its existing hover styling; the diff card adds no motion of its own.
    const clone = host.cloneNode(true) as HTMLElement;
    for (const node of all(clone, "raw-command-disclosure")) node.remove();
    const html = clone.innerHTML;
    expect(html).not.toMatch(/border-r\b|border-right|animate-|transition/);
    const theme = themeToCSS(DEFAULT_THEME) as Record<string, unknown>;
    const tokens = [...new Set(html.match(/--ct-[a-z-]+/g) ?? [])];
    expect(tokens.length).toBeGreaterThan(0);
    expect(tokens.filter((token) => !(token in theme))).toEqual([]);
  });
});
