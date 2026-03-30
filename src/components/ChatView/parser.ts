/**
 * Parsers for Claude Code chat messages.
 *
 * Primary: JSONL session file parser (structured, reliable).
 * Fallback: PTY terminal output parser (legacy, for panes without session ID).
 */

export type MessageType =
  | "user"
  | "assistant"
  | "tool_call"
  | "tool_result"
  | "thinking"
  | "status"
  | "image";

export interface ImageAttachment {
  id: string;
  mimeType: string;
  data?: string;           // base64 data (omitted for large images)
}

export interface ChatMessage {
  id: string;
  type: MessageType;
  content: string;
  toolName?: string;
  toolArgs?: string;
  toolInput?: Record<string, unknown>; // full tool input (e.g. Write content)
  toolId?: string;
  timestamp?: string;
  collapsed?: boolean;
  duration?: string;
  imageData?: string;      // base64 data (for standalone image messages)
  imageMimeType?: string;  // e.g. "image/png"
  attachments?: ImageAttachment[]; // images attached to user messages
}

// ---------------------------------------------------------------------------
// JSONL session parser (primary)
// ---------------------------------------------------------------------------

/**
 * Parse Claude Code JSONL session data into structured chat messages.
 * Each line is a JSON object with a `type` field.
 */
export function parseJsonlSession(jsonlText: string): ChatMessage[] {
  const messages: ChatMessage[] = [];
  const lines = jsonlText.split("\n").filter((l) => l.trim());

  for (const line of lines) {
    try {
      const entry = JSON.parse(line);

      const role =
        entry.type === "user" || entry.type === "assistant"
          ? entry.type
          : entry.type === "message"
            ? entry.message?.role
            : undefined;

      if (role === "user") {
        parseUserEntry(entry, messages);
      } else if (role === "assistant") {
        parseAssistantBlocks(entry, messages);
      }
      // Skip: system, progress, file-history-snapshot, queue-operation, last-prompt
    } catch {
      // Skip malformed lines
    }
  }

  return messages;
}

function parseUserEntry(
  entry: Record<string, unknown>,
  messages: ChatMessage[],
): void {
  const msg = entry.message as Record<string, unknown> | undefined;
  const rawContent = msg?.content;
  const uuid = entry.uuid as string | undefined;
  const timestamp = entry.timestamp as string | undefined;

  if (Array.isArray(rawContent)) {
    const textParts: string[] = [];
    const attachments: ImageAttachment[] = [];
    for (const block of rawContent) {
      if (block.type === "text" && block.text?.trim()) {
        textParts.push(block.text.trim());
      } else if (block.type === "image") {
        const source = block.source as Record<string, unknown> | undefined;
        const mimeType = (source?.media_type as string) || "image/png";
        const data = source?.data as string | undefined;
        attachments.push({
          id: `${uuid || "user"}-img-${attachments.length}`,
          mimeType,
          data: data && data.length < 100_000 ? data : undefined,
        });
      }
    }
    const content = textParts.join("\n");
    if (content || attachments.length > 0) {
      messages.push({
        id: `${uuid || "user"}-text-${messages.length}`,
        type: "user",
        content: content || (attachments.length > 0 ? `[${attachments.length} image${attachments.length > 1 ? "s" : ""}]` : ""),
        timestamp,
        attachments: attachments.length > 0 ? attachments : undefined,
      });
    }
  } else {
    const content = extractText(rawContent);
    if (content.trim()) {
      messages.push({
        id: (uuid as string) || `user-${messages.length}`,
        type: "user",
        content: content.trim(),
        timestamp,
      });
    }
  }
}

function parseAssistantBlocks(
  entry: Record<string, unknown>,
  messages: ChatMessage[],
): void {
  const msg = entry.message as Record<string, unknown> | undefined;
  const contentBlocks = msg?.content;
  const uuid = entry.uuid as string | undefined;
  const timestamp = entry.timestamp as string | undefined;

  if (!Array.isArray(contentBlocks)) return;

  for (const block of contentBlocks) {
    if (block.type === "text" && block.text?.trim()) {
      messages.push({
        id: `${uuid}-text-${messages.length}`,
        type: "assistant",
        content: block.text.trim(),
        timestamp,
      });
    } else if (block.type === "tool_use") {
      messages.push({
        id: block.id || `tool-${messages.length}`,
        type: "tool_call",
        content: `${block.name}(${summarizeInput(block.input)})`,
        toolName: block.name,
        toolArgs: summarizeInput(block.input),
        toolInput: block.input ?? undefined,
        toolId: block.id,
        collapsed: true,
        timestamp,
      });
    } else if (block.type === "tool_result") {
      const resultText =
        typeof block.content === "string"
          ? block.content
          : Array.isArray(block.content)
            ? block.content
                .map((c: Record<string, unknown>) => (c.text as string) || "")
                .join("\n")
            : JSON.stringify(block.content);
      messages.push({
        id: `result-${block.tool_use_id || messages.length}`,
        type: "tool_result",
        content:
          resultText.slice(0, 500) +
          (resultText.length > 500 ? "..." : ""),
        toolId: block.tool_use_id,
        collapsed: true,
        timestamp,
      });
    } else if (block.type === "image") {
      const imgMsg = parseImageBlock(block, uuid || "", messages.length, timestamp);
      if (imgMsg) messages.push(imgMsg);
    } else if (block.type === "thinking" && block.text?.trim()) {
      messages.push({
        id: `think-${messages.length}`,
        type: "thinking",
        content:
          block.text.trim().slice(0, 200) +
          (block.text.length > 200 ? "..." : ""),
        collapsed: true,
        timestamp,
      });
    }
  }
}

/**
 * Parse an image content block into a ChatMessage.
 * Handles base64 data, URL references, and file path references.
 */
function parseImageBlock(
  block: Record<string, unknown>,
  uuid: string,
  index: number,
  timestamp?: string,
): ChatMessage | null {
  const source = block.source as Record<string, unknown> | undefined;
  if (!source) return null;

  // base64-encoded image data
  if (source.data) {
    return {
      id: `${uuid}-img-${index}`,
      type: "image",
      content: "",
      imageData: source.data as string,
      imageMimeType: (source.media_type as string) || "image/png",
      timestamp,
    };
  }

  // URL or file-path reference
  const ref = (source.url || source.path) as string | undefined;
  if (ref) {
    const isFilePath = ref.startsWith("/") || ref.startsWith("file://");
    return {
      id: `${uuid}-img-${index}`,
      type: "image",
      content: "",
      imageData: isFilePath ? `file://${ref.replace(/^file:\/\//, "")}` : ref,
      imageMimeType: (source.media_type as string) || "image/png",
      timestamp,
    };
  }

  // Fallback: image block exists but no renderable source — show placeholder
  return {
    id: `${uuid}-img-${index}`,
    type: "image",
    content: "",
    timestamp,
  };
}

function extractText(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .filter((c: Record<string, unknown>) => c.type === "text")
      .map((c: Record<string, unknown>) => (c.text as string) || "")
      .join("\n");
  }
  return "";
}

function summarizeInput(input: Record<string, unknown> | null | undefined): string {
  if (!input) return "";
  if (input.file_path) return input.file_path as string;
  if (input.command) return (input.command as string).slice(0, 80);
  if (input.pattern) return input.pattern as string;
  if (input.prompt) return (input.prompt as string).slice(0, 60) + "...";
  if (input.description) return (input.description as string).slice(0, 60);
  return JSON.stringify(input).slice(0, 60);
}

// ---------------------------------------------------------------------------
// Terminal output parser (fallback)
// ---------------------------------------------------------------------------

// Known tool names that appear after the marker
const TOOL_NAMES = new Set([
  "Bash",
  "Read",
  "Write",
  "Edit",
  "Agent",
  "Grep",
  "Glob",
  "Skill",
  "Update",
  "TodoWrite",
  "TodoRead",
  "WebFetch",
  "WebSearch",
  "ToolSearch",
  "NotebookEdit",
]);

// Unicode markers
const USER_MARKER = "\u276F"; // >
const ASSISTANT_MARKER = "\u23FA"; // rec
const STATUS_MARKER = "\u273B"; // *
const SUB_ITEM_MARKER = "\u23BF"; // sub

/**
 * Parse Claude Code terminal output into structured chat messages.
 * Legacy fallback when no JSONL session is available.
 */
export function parseTerminalOutput(text: string): ChatMessage[] {
  if (!text || !text.trim()) return [];

  const lines = text.split("\n");
  const messages: ChatMessage[] = [];

  let currentBlock: {
    startLine: number;
    lines: string[];
    marker: string;
  } | null = null;

  function flushBlock() {
    if (!currentBlock || currentBlock.lines.length === 0) return;

    const { startLine, lines: blockLines, marker } = currentBlock;
    const rawContent = blockLines.join("\n").trim();
    if (!rawContent) return;

    const id = `msg-${startLine}`;

    if (marker === USER_MARKER) {
      const firstLine = blockLines[0].replace(/^\s*\u276F\s*/, "");
      const rest = blockLines.slice(1).join("\n").trim();
      const content = rest ? `${firstLine}\n${rest}` : firstLine;
      messages.push({ id, type: "user", content });
    } else if (marker === STATUS_MARKER) {
      const content = blockLines[0].replace(/^\s*\u273B\s*/, "").trim();
      const durationMatch = content.match(/(\d+[smh]\b.*)/);
      messages.push({
        id,
        type: "status",
        content,
        duration: durationMatch ? durationMatch[1] : undefined,
      });
    } else if (marker === ASSISTANT_MARKER) {
      const firstLine = blockLines[0].replace(/^\s*\u23FA\s*/, "");

      const toolMatch = firstLine.match(/^(\w+)\s*\(/);
      const toolNameOnly = firstLine.match(/^(\w+)\s*$/);

      if (toolMatch && TOOL_NAMES.has(toolMatch[1])) {
        const toolName = toolMatch[1];
        const argsStart = firstLine.indexOf("(");
        let toolArgs = firstLine.slice(argsStart + 1);
        if (toolArgs.endsWith(")")) {
          toolArgs = toolArgs.slice(0, -1);
        }
        const restLines = blockLines.slice(1);
        if (restLines.length > 0) {
          toolArgs = toolArgs + "\n" + restLines.join("\n");
        }
        toolArgs = toolArgs.replace(/\)\s*$/, "").trim();

        messages.push({
          id,
          type: "tool_call",
          content: rawContent,
          toolName,
          toolArgs,
          collapsed: true,
        });
      } else if (toolNameOnly && TOOL_NAMES.has(toolNameOnly[1])) {
        const toolName = toolNameOnly[1];
        const toolArgs = blockLines.slice(1).join("\n").trim();
        messages.push({
          id,
          type: "tool_call",
          content: rawContent,
          toolName,
          toolArgs,
          collapsed: true,
        });
      } else {
        const rest = blockLines.slice(1).join("\n").trim();
        const content = rest ? `${firstLine}\n${rest}` : firstLine;
        messages.push({ id, type: "assistant", content });
      }
    }
  }

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const trimmed = line.trimStart();

    if (trimmed.startsWith(USER_MARKER)) {
      flushBlock();
      currentBlock = { startLine: i, lines: [line], marker: USER_MARKER };
    } else if (trimmed.startsWith(ASSISTANT_MARKER)) {
      flushBlock();
      currentBlock = {
        startLine: i,
        lines: [line],
        marker: ASSISTANT_MARKER,
      };
    } else if (trimmed.startsWith(STATUS_MARKER)) {
      flushBlock();
      currentBlock = { startLine: i, lines: [line], marker: STATUS_MARKER };
    } else if (trimmed.startsWith(SUB_ITEM_MARKER)) {
      flushBlock();
      const content = line.replace(/^\s*\u23BF\s*/, "").trim();
      const subLines = [content];
      while (i + 1 < lines.length) {
        const nextTrimmed = lines[i + 1].trimStart();
        if (
          nextTrimmed.startsWith(USER_MARKER) ||
          nextTrimmed.startsWith(ASSISTANT_MARKER) ||
          nextTrimmed.startsWith(STATUS_MARKER)
        ) {
          break;
        }
        if (nextTrimmed.startsWith(SUB_ITEM_MARKER)) {
          subLines.push(nextTrimmed.replace(/^\u23BF\s*/, ""));
        } else if (nextTrimmed) {
          subLines.push(lines[i + 1]);
        }
        i++;
      }

      messages.push({
        id: `msg-${i}-result`,
        type: "tool_result",
        content: subLines.join("\n").trim(),
        collapsed: true,
      });
      currentBlock = null;
    } else if (currentBlock) {
      currentBlock.lines.push(line);
    }
  }

  flushBlock();
  return messages;
}

// Keep old name as alias for backward compat
export const parseClaudeOutput = parseTerminalOutput;
