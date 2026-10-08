import { describe, expect, it } from "vitest";
import { formatAttachmentMarker } from "./attachmentMarker";
import {
  attachmentTagsAsLabels,
  clipMessage,
  markdownPlainText,
  messageClipboard,
  messageMarkdownClipboard,
  messagesClipboard,
  messagesMarkdown,
  messagesQuote,
  messagesExcerpt,
  messagesReference,
  messagesReplyLabel,
  sourceDiffersFromVisible,
  toolEventText,
  type ClipMessage,
} from "./messageClipboard";
import { materializeToolEvent } from "./toolEvents";
import type { NormalizedChatMessage } from "./types";

const IMAGE = formatAttachmentMarker("C:\\tmp\\shot.png", "image", "image1");
const user = (source: string): ClipMessage => ({ id: "u", role: "user", author: "You", time: "14:05", source });
const agent = (source: string): ClipMessage => ({ id: "a", role: "assistant", author: "Fermi", time: "14:06", source });
const names = { agent: "Fermi", formatTime: (ts: NormalizedChatMessage["timestamp"]) => (ts ? "14:05" : undefined) };

describe("attachment tags", () => {
  it("become their capsule labels, or the file name when unlabelled", () => {
    const unlabelled = formatAttachmentMarker("/docs/plan.md", "file");
    expect(attachmentTagsAsLabels(`see ${IMAGE} and ${unlabelled}.`)).toBe("see image1 and plan.md.");
  });

  it("copy keeps the exact source with tags in the portable flavour and labels in plain", () => {
    const source = `look ${IMAGE} **now**`;
    expect(messageClipboard(user(source))).toEqual({ plain: "look image1 **now**", portable: source });
  });
});

describe("markdownPlainText", () => {
  it("removes inline and block markup and keeps code verbatim", () => {
    const markdown = [
      "## Title",
      "Some **bold**, *em*, _under_ and ~~gone~~ with `a*b*c` and snake_case_name.",
      "> quoted",
      "- item",
      "- [x] done",
      "See [docs](https://x.dev) or <https://y.dev>.",
      "```ts",
      "const **x** = 1;",
      "```",
      "---",
    ].join("\n");
    expect(markdownPlainText(markdown)).toBe([
      "Title",
      "Some bold, em, under and gone with a*b*c and snake_case_name.",
      "quoted",
      "• item",
      "☑ done",
      "See docs (https://x.dev) or https://y.dev.",
      "const **x** = 1;",
      "",
    ].join("\n"));
  });

  it("leaves text without markup unchanged", () => {
    expect(markdownPlainText("plain words, nothing else")).toBe("plain words, nothing else");
  });
});

describe("Copy as Markdown", () => {
  it("is offered only when the visible text differs from the source", () => {
    expect(sourceDiffersFromVisible(agent("plain answer"))).toBe(false);
    expect(sourceDiffersFromVisible(agent("**bold** answer"))).toBe(true);
    expect(sourceDiffersFromVisible(user(`with ${IMAGE}`))).toBe(true);
    expect(sourceDiffersFromVisible(user("user *stars* stay as typed"))).toBe(false);
  });

  it("copies the exact source in both flavours", () => {
    const source = `# Head\n${IMAGE}`;
    expect(messageMarkdownClipboard(agent(source))).toEqual({ plain: source, portable: source });
  });
});

describe("several messages", () => {
  const messages = [user(`fix ${IMAGE}`), agent("Done **now**.")];

  it("copy concatenates in order under role headings", () => {
    expect(messagesClipboard(messages)).toEqual({
      plain: "You · 14:05\nfix image1\n\nFermi · 14:06\nDone now.",
      portable: `### You · 14:05\n\nfix ${IMAGE}\n\n### Fermi · 14:06\n\nDone **now**.`,
    });
  });

  it("one selected message copies as itself", () => {
    expect(messagesClipboard([messages[1]])).toEqual(messageClipboard(messages[1]));
  });

  it("quote attributes each block and references them", () => {
    expect(messagesQuote([agent("line one\n\nline two\n")])).toBe("Fermi · 14:06:\n> line one\n>\n> line two");
    expect(messagesReference([messages[1]])).toBe("Fermi 14:06");
    expect(messagesReference(messages)).toBe("2 messages");
    expect(messagesMarkdown([{ ...messages[1], time: undefined }])).toBe("### Fermi\n\nDone **now**.");
  });

  it("a reply names who and when, or how many, with one line of what a reader sees", () => {
    expect(messagesReplyLabel([messages[1]])).toBe("Fermi · 14:06");
    expect(messagesReplyLabel([{ ...messages[1], time: undefined }])).toBe("Fermi");
    expect(messagesReplyLabel(messages)).toBe("2 messages");
    expect(messagesExcerpt([agent("**Done**\n\nnext   step"), user("later")])).toBe("Done next step");
    const long = messagesExcerpt([user("x".repeat(400))]);
    expect(long.length).toBe(160);
    expect(long.endsWith("…")).toBe(true);
  });

  it("tool rows carry a Tool heading", () => {
    const tool: ClipMessage = { id: "t", role: "tool", author: "Bash", source: "$ ls\na" };
    expect(messagesClipboard([tool, messages[1]]).plain).toBe("Tool · Bash\n$ ls\na\n\nFermi · 14:06\nDone now.");
  });
});

describe("tool rows", () => {
  const call = (overrides: Partial<NormalizedChatMessage>): NormalizedChatMessage => ({
    id: "c", type: "tool_call", content: "", toolName: "Bash", toolKind: "command", ...overrides,
  });

  it("copy a command and its output, not decorations", () => {
    const event = materializeToolEvent(call({ toolSummary: "List files", toolInput: { command: "ls -la" }, toolResult: "a\nb\n" }))!;
    expect(toolEventText(event)).toBe("$ ls -la\na\nb");
  });

  it("copy a non-command call's arguments and output", () => {
    const event = materializeToolEvent(call({ toolName: "Grep", toolKind: "search", toolSummary: "needle", toolInput: { pattern: "needle", path: undefined }, toolResult: "x.ts:1" }))!;
    expect(toolEventText(event)).toBe('Grep: needle\n{\n  "pattern": "needle"\n}\nx.ts:1');
  });

  it("copy an edit as its unified diff", () => {
    const event = materializeToolEvent(call({
      toolName: "Edit", toolKind: "edit", toolSummary: "Edit a.ts", toolInput: { file_path: "a.ts", old_string: "x", new_string: "y" },
      fileDiffs: [{ path: "a.ts", additions: 1, deletions: 1, hunks: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-x", "+y"] }] }],
    }))!;
    expect(toolEventText(event)).toBe("Edit a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1,1 +1,1 @@\n-x\n+y");
  });

  it("clipMessage picks role, author and source per row kind", () => {
    const toolMessage = call({ toolInput: { command: "pwd" }, toolResult: "/w" });
    expect(clipMessage(toolMessage, materializeToolEvent(toolMessage)!, names)).toMatchObject({ role: "tool", author: "Bash", source: "$ pwd\n/w" });
    expect(clipMessage({ id: "s", type: "shell_command", content: "out\n", shellCommand: "echo hi" }, undefined, names)).toMatchObject({ role: "tool", author: "Shell", source: "$ echo hi\nout" });
    expect(clipMessage({ id: "u", type: "user", content: "hi", timestamp: "2026-01-01T14:05:00Z" }, undefined, names)).toEqual({ id: "u", role: "user", author: "You", time: "14:05", source: "hi" });
    expect(clipMessage({ id: "a", type: "assistant", content: "ok" }, undefined, names)).toEqual({ id: "a", role: "assistant", author: "Fermi", source: "ok" });
  });
});
