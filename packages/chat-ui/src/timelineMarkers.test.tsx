import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { describe, expect, it } from "vitest";
import {
  reduceToolEvents,
  type NormalizedChatMessage,
} from "@codeterm/chat-core";
import claudePlugin from "../../../plugins/provider-claude/src/plugin";
import codexPlugin from "../../../plugins/provider-codex/src/plugin";
import { MessageBubble } from "./MessageBubble";
import {
  ChatSessionIntentLine,
  sessionIntentFromMessages,
} from "./timelineMarkers";

type RichMessage = Record<string, unknown> & {
  blocks?: Array<Record<string, unknown>>;
};

function jsonLines(rows: unknown[]): string {
  return rows.map((row) => JSON.stringify(row)).join("\n");
}

function parsePlugin(
  plugin: { parseSessionDelta?: (chunk: string) => unknown },
  chunk: string,
): RichMessage[] {
  const parsed = plugin.parseSessionDelta?.(chunk) as { messages?: unknown[] } | undefined;
  return (parsed?.messages ?? []).filter(
    (message): message is RichMessage => message !== null && typeof message === "object" && !Array.isArray(message),
  );
}

function richText(message: RichMessage): string {
  return (message.blocks ?? [])
    .map((block) => {
      const data = block.data;
      const record = data && typeof data === "object" && !Array.isArray(data)
        ? data as Record<string, unknown>
        : null;
      return record && typeof record.text === "string"
        ? record.text
        : "";
    })
    .filter(Boolean)
    .join("\n");
}

function normalizedMessages(messages: RichMessage[]): NormalizedChatMessage[] {
  return messages.map((message, index) => ({
    id: typeof message.uuid === "string" ? message.uuid : `message-${index}`,
    type: (message.role === "custom" ? "custom" : message.role) as NormalizedChatMessage["type"],
    content: richText(message),
    seq: index,
    timelineMarker: message.timelineMarker as NormalizedChatMessage["timelineMarker"],
    sessionIntent: message.sessionIntent as NormalizedChatMessage["sessionIntent"],
  }));
}

function withMarkerContent(message: NormalizedChatMessage): NormalizedChatMessage {
  if (message.timelineMarker?.kind === "context_compaction") {
    return { ...message, type: "system", content: "context compacted" };
  }
  if (message.timelineMarker?.kind === "external_edit") {
    return { ...message, type: "system", content: `changed outside: ${message.timelineMarker.path}` };
  }
  return message;
}

async function mountMessages(messages: NormalizedChatMessage[]): Promise<{ host: HTMLElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <div>
        {reduceToolEvents(messages).map((item) => {
          const message = item.kind === "message" ? item.message : item.event.message;
          return <MessageBubble key={message.id} message={message} item={item.kind === "tool" ? item : undefined} />;
        })}
      </div>,
    );
  });
  return { host, root };
}

function unmount(host: HTMLElement, root: Root): void {
  root.unmount();
  host.remove();
}

describe("timeline markers", () => {
  it("normalizes Claude queued, outside-edit, compact, and title records", () => {
    const rows = parsePlugin(claudePlugin, `${jsonLines([
      { type: "user", uuid: "before", message: { role: "user", content: "before" } },
      {
        type: "attachment",
        uuid: "queued",
        attachment: { type: "queued_command", prompt: "queued input" },
      },
      { type: "edited_text_file", uuid: "edit", filename: "notes.md" },
      { type: "user", uuid: "compact", isCompactSummary: true, message: { role: "user", content: "old summary" } },
      { type: "system", uuid: "boundary", subtype: "compact_boundary", content: "" },
      { type: "ai-title", uuid: "title", aiTitle: "Release notes" },
      { type: "ai-title", uuid: "bad-title", aiTitle: { value: "not text" } },
      { type: "edited_text_file", uuid: "bad-edit", filename: { value: "not a path" } },
    ])}\n{invalid`);
    const normalized = normalizedMessages(rows);

    expect(normalized.map((message) => message.id)).toEqual([
      "before", "queued", "edit", "compact", "boundary", "title",
    ]);
    expect(normalized[1]?.timelineMarker).toEqual({ kind: "queued" });
    expect(normalized[2]?.timelineMarker).toEqual({ kind: "external_edit", path: "notes.md" });
    expect(normalized[3]?.timelineMarker).toEqual({ kind: "context_compaction" });
    expect(normalized[4]?.timelineMarker).toEqual({ kind: "context_compaction" });
    expect(normalized[5]?.sessionIntent).toEqual({ title: "Release notes" });
    expect(normalized[0]?.content).toBe("before");
  });

  it("normalizes Codex compaction and goal records while isolating malformed rows", () => {
    const rows = parsePlugin(codexPlugin, jsonLines([
      { type: "response_item", payload: { type: "message", role: "assistant", content: "before" } },
      { type: "event_msg", payload: { type: "context_compacted" } },
      { type: "event_msg", payload: { type: "thread_goal_updated", goal: "Ship marker support" } },
      { type: "event_msg", payload: { type: "thread_goal_updated", goal: { value: "not text" } } },
      { type: "response_item", payload: { type: "message", role: "assistant", content: "after" } },
      "{invalid",
    ]));
    const normalized = normalizedMessages(rows);

    expect(normalized.map((message) => message.content)).toEqual([
      "before", "context compacted", "", "after",
    ]);
    expect(normalized[1]?.timelineMarker).toEqual({ kind: "context_compaction" });
    expect(normalized[2]?.sessionIntent).toEqual({ goal: "Ship marker support" });
    expect(normalized[0]?.content).toBe("before");
    expect(normalized[3]?.content).toBe("after");
  });

  it("renders queued input without a marker and changed-outside rows in order", async () => {
    const messages: NormalizedChatMessage[] = [
      { id: "one", type: "user", content: "one", seq: 0 },
      { id: "queued", type: "user", content: "queued input", seq: 1, timelineMarker: { kind: "queued" } },
      { id: "edit", type: "system", content: "changed outside: notes.md", seq: 2, timelineMarker: { kind: "external_edit", path: "notes.md" } },
      { id: "two", type: "assistant", content: "two", seq: 3 },
    ];
    const { host, root } = await mountMessages(messages);
    try {
      expect(host.textContent).toContain("one");
      expect(host.textContent).toContain("queued input");
      expect(host.textContent).toContain("changed outside: notes.md");
      expect(host.textContent).toContain("two");
      expect(host.querySelector('[data-testid="chat-queued-marker"]')).toBeNull();
      expect(host.querySelector('[data-testid="chat-external-edit-row"]')).not.toBeNull();
      const rendered = host.textContent ?? "";
      expect(rendered.indexOf("one")).toBeLessThan(rendered.indexOf("queued"));
      expect(rendered.indexOf("queued")).toBeLessThan(rendered.indexOf("changed outside: notes.md"));
      expect(rendered.indexOf("changed outside: notes.md")).toBeLessThan(rendered.indexOf("two"));
    } finally {
      unmount(host, root);
    }
  });

  it("renders equivalent normalized Claude and Codex markers identically", async () => {
    const claude = withMarkerContent({
      id: "claude-context",
      type: "system",
      content: "context compacted",
      timelineMarker: { kind: "context_compaction" },
    });
    const codex = withMarkerContent({
      id: "codex-context",
      type: "system",
      content: "context compacted",
      timelineMarker: { kind: "context_compaction" },
    });
    const first = await mountMessages([claude]);
    const second = await mountMessages([codex]);
    try {
      expect(first.host.textContent).toBe(second.host.textContent);
      expect(first.host.querySelector('[data-testid="chat-context-compacted-divider"]')).not.toBeNull();
      expect(second.host.querySelector('[data-testid="chat-context-compacted-divider"]')).not.toBeNull();
    } finally {
      unmount(first.host, first.root);
      unmount(second.host, second.root);
    }
  });

  it("collapses adjacent compact rows to one ordered divider", async () => {
    const messages = [
      { id: "before", type: "assistant", content: "before", seq: 0 },
      { id: "summary", type: "system", content: "old summary", seq: 1, timelineMarker: { kind: "context_compaction" as const } },
      { id: "boundary", type: "system", content: "context compacted", seq: 2, timelineMarker: { kind: "context_compaction" as const } },
      { id: "after", type: "assistant", content: "after", seq: 3 },
    ] satisfies NormalizedChatMessage[];
    const { host, root } = await mountMessages(messages.map(withMarkerContent));
    try {
      expect(host.querySelectorAll('[data-testid="chat-context-compacted-divider"]').length).toBe(1);
      expect(host.textContent).toContain("before");
      expect(host.textContent).toContain("after");
    } finally {
      unmount(host, root);
    }
  });

  it("preserves the last valid title and goal and ignores arbitrary prose", async () => {
    const messages: NormalizedChatMessage[] = [
      { id: "prose", type: "assistant", content: "arbitrary prose that is not a goal" },
      { id: "title", type: "custom", content: "", sessionIntent: { title: "First title" } },
      { id: "goal", type: "custom", content: "", sessionIntent: { goal: "Ship the feature" } },
      { id: "bad-title", type: "custom", content: "", sessionIntent: { title: "\u0000bad" } },
      { id: "bad-goal", type: "custom", content: "", sessionIntent: { goal: "" } },
    ];
    expect(sessionIntentFromMessages(messages)).toEqual({ title: "First title", goal: "Ship the feature" });

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    try {
      await act(async () => {
        root.render(<ChatSessionIntentLine intent={sessionIntentFromMessages(messages)} />);
      });
      expect(host.querySelector('[data-testid="chat-session-title"]')?.textContent).toContain("First title");
      expect(host.querySelector('[data-testid="chat-session-goal"]')?.textContent).toContain("Ship the feature");
      expect(host.textContent).not.toContain("arbitrary prose");
    } finally {
      unmount(host, root);
    }
  });
});
