import type { NormalizedChatMessage } from "@codeterm/chat-core";

export type CodexChatMessage = NormalizedChatMessage;

export function parseJsonlSession(jsonlText: string): CodexChatMessage[] {
  const messages: CodexChatMessage[] = [];
  const lines = jsonlText.split("\n").filter((line) => line.trim());

  for (const line of lines) {
    let entry: Record<string, unknown>;
    try {
      entry = JSON.parse(line) as Record<string, unknown>;
    } catch {
      continue;
    }
    parseEntry(entry, messages);
  }

  return messages;
}

function parseEntry(entry: Record<string, unknown>, messages: CodexChatMessage[]): void {
  const type = entry.type;
  const payload = entry.payload as Record<string, unknown> | undefined;
  const timestamp = entry.timestamp as string | undefined;

  if (type === "response_item" && payload) {
    parseResponsePayload(payload, timestamp, messages);
    return;
  }

  if (type === "event_msg" && payload?.type === "agent_message") {
    const message = extractText(payload.message);
    if (message.trim()) {
      pushMessage(messages, {
        id: idFor(entry, messages, "assistant"),
        type: "assistant",
        content: message.trim(),
        timestamp,
        provider: "codex",
      });
    }
    return;
  }

  if (type === "error") {
    const message = extractText(payload?.message ?? entry.message);
    if (message.trim()) {
      messages.push({
        id: idFor(entry, messages, "status"),
        type: "status",
        content: message.trim(),
        timestamp,
        provider: "codex",
      });
    }
  }
}

function parseResponsePayload(
  payload: Record<string, unknown>,
  timestamp: string | undefined,
  messages: CodexChatMessage[],
): void {
  if (payload.type === "message") {
    const role = payload.role;
    if (role !== "user" && role !== "assistant") return;
    const content = parseContentBlocks(payload.content);
    if (!content.trim()) return;
    if (role === "user") {
      if (isCodexBootstrapInstruction(content)) return;
      pushUserContentWithContext(content, payload, timestamp, messages);
      return;
    }
    pushMessage(messages, {
      id: idFor(payload, messages, role),
      type: role,
      content: content.trim(),
      timestamp,
      provider: "codex",
    });
    return;
  }

  if (payload.type === "reasoning") {
    const summary = Array.isArray(payload.summary)
      ? payload.summary.map(extractText).filter(Boolean).join("\n")
      : "";
    const content = extractText(payload.content) || summary;
    if (content.trim()) {
      messages.push({
        id: idFor(payload, messages, "thinking"),
        type: "thinking",
        content: content.trim(),
        timestamp,
        provider: "codex",
        collapsed: true,
      });
    }
    return;
  }

  if (payload.type === "custom_tool_call" || payload.type === "function_call") {
    const name = extractText(payload.name) || "tool";
    const input = payload.input;
    messages.push({
      id: extractText(payload.call_id) || idFor(payload, messages, "tool"),
      type: "tool_call",
      content: `${name}(${summarizeInput(input)})`,
      toolName: name,
      toolArgs: summarizeInput(input),
      toolInput: isRecord(input) ? input : undefined,
      toolId: extractText(payload.call_id),
      timestamp,
      provider: "codex",
      collapsed: true,
    });
    return;
  }

  if (payload.type === "custom_tool_call_output" || payload.type === "function_call_output") {
    const output = extractText(payload.output);
    if (!output.trim()) return;
    messages.push({
      id: `result-${extractText(payload.call_id) || messages.length}`,
      type: "tool_result",
      content: output,
      toolId: extractText(payload.call_id),
      timestamp,
      provider: "codex",
      collapsed: true,
    });
  }
}

function pushMessage(messages: CodexChatMessage[], message: CodexChatMessage): void {
  const duplicate = messages
    .slice(-6)
    .some((existing) =>
      existing.type === message.type &&
      existing.provider === message.provider &&
      existing.content === message.content
    );
  if (!duplicate) {
    messages.push(message);
  }
}

function pushUserContentWithContext(
  content: string,
  payload: Record<string, unknown>,
  timestamp: string | undefined,
  messages: CodexChatMessage[],
): void {
  const split = splitLeadingEnvironmentContext(content);
  if (split.environmentContext) {
    messages.push({
      id: `${idFor(payload, messages, "system")}-environment`,
      type: "system",
      content: split.environmentContext.trim(),
      timestamp,
      provider: "codex",
      tagName: "environment_context",
      systemSubtype: "environment_context",
    });
  }
  if (split.userText.trim()) {
    messages.push({
      id: idFor(payload, messages, "user"),
      type: "user",
      content: split.userText.trim(),
      timestamp,
      provider: "codex",
    });
  }
}

function splitLeadingEnvironmentContext(content: string): {
  environmentContext: string | null;
  userText: string;
} {
  const match = content.match(/^\s*<environment_context>([\s\S]*?)<\/environment_context>\s*/);
  if (!match) {
    return { environmentContext: null, userText: content };
  }
  return {
    environmentContext: match[1],
    userText: content.slice(match[0].length),
  };
}

function isCodexBootstrapInstruction(content: string): boolean {
  const trimmed = content.trimStart();
  return (
    trimmed.startsWith("# AGENTS.md instructions for ") &&
    trimmed.includes("<INSTRUCTIONS>") &&
    trimmed.includes("</INSTRUCTIONS>")
  );
}

function parseContentBlocks(content: unknown): string {
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  const chunks: string[] = [];
  for (const block of content) {
    if (!isRecord(block)) continue;
    if (
      (block.type === "input_text" ||
        block.type === "output_text" ||
        block.type === "text") &&
      typeof block.text === "string"
    ) {
      chunks.push(block.text);
    }
  }
  return chunks.join("\n");
}

function summarizeInput(input: unknown): string {
  if (typeof input === "string") return input.length > 240 ? `${input.slice(0, 240)}...` : input;
  if (input == null) return "";
  try {
    const serialized = JSON.stringify(input);
    return serialized.length > 240 ? `${serialized.slice(0, 240)}...` : serialized;
  } catch {
    return String(input);
  }
}

function extractText(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function idFor(entry: Record<string, unknown>, messages: CodexChatMessage[], prefix: string): string {
  return extractText(entry.id) || extractText(entry.uuid) || `${prefix}-${messages.length}`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}
