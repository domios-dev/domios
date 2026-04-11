export type ClaudeStreamEvent =
  | {
      type: "system";
      subtype?: string;
      session_id?: string;
      model?: string;
      tools?: string[];
      raw: unknown;
    }
  | {
      type: "assistant";
      message_id: string;
      content: ClaudeContentBlock[];
      usage?: { input_tokens: number; output_tokens: number };
      raw: unknown;
    }
  | {
      type: "user";
      message_id?: string;
      content: ClaudeContentBlock[];
      raw: unknown;
    }
  | {
      type: "result";
      subtype?: string;
      is_error: boolean;
      duration_ms?: number;
      total_cost_usd?: number;
      result?: string;
      stop_reason?: string;
      raw: unknown;
    }
  | {
      type: "rate_limit";
      raw: unknown;
    }
  | {
      type: "unknown";
      kind: string;
      raw: unknown;
    };

export type ClaudeContentBlock =
  | { type: "text"; text: string }
  | { type: "thinking"; thinking: string; signature?: string }
  | { type: "tool_use"; id: string; name: string; input: unknown }
  | {
      type: "tool_result";
      tool_use_id: string;
      content: unknown;
      is_error?: boolean;
    }
  | { type: "unknown"; raw: unknown };

export interface RawLine {
  text: string;
}

export interface ParseResult {
  events: ClaudeStreamEvent[];
  rawLines: RawLine[];
}

// Strip CSI (`ESC [ ... letter`), OSC (`ESC ] ... BEL` or `ESC ] ... ESC \`),
// and standalone control bytes (BEL, BS, etc) that PowerShell + claude PTY
// rendering interleave with the stream-json stdout.
const ANSI_PATTERN =
  /\x1b\[[0-9;?]*[a-zA-Z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|[\x00-\x08\x0b-\x1f\x7f]/g;

export class ClaudeStreamParser {
  private buffer = "";

  feed(chunk: string): ParseResult {
    this.buffer += chunk;
    const events: ClaudeStreamEvent[] = [];
    const rawLines: RawLine[] = [];

    const cleaned = this.buffer.replace(ANSI_PATTERN, "");
    const lines = cleaned.split(/\r?\n/);
    this.buffer = lines.pop() ?? "";

    for (const rawLine of lines) {
      const line = rawLine.trim();
      if (!line) continue;

      // Find the first JSON object on the line — claude stream-json sometimes
      // has prompt echo or trailing text on the same line as the event.
      const start = line.indexOf("{");
      const end = line.lastIndexOf("}");
      if (start !== -1 && end > start) {
        const candidate = line.slice(start, end + 1);
        try {
          const parsed = JSON.parse(candidate);
          const ev = mapEvent(parsed);
          if (ev) {
            events.push(ev);
            continue;
          }
        } catch {
          // Fall through to raw.
        }
      }
      rawLines.push({ text: rawLine });
    }

    return { events, rawLines };
  }

  reset() {
    this.buffer = "";
  }
}

function mapEvent(obj: unknown): ClaudeStreamEvent | null {
  if (!obj || typeof obj !== "object") return null;
  const o = obj as Record<string, unknown>;
  const t = typeof o.type === "string" ? o.type : "";

  if (t === "system") {
    return {
      type: "system",
      subtype: typeof o.subtype === "string" ? o.subtype : undefined,
      session_id: typeof o.session_id === "string" ? o.session_id : undefined,
      model: typeof o.model === "string" ? o.model : undefined,
      tools: Array.isArray(o.tools) ? (o.tools as string[]) : undefined,
      raw: obj,
    };
  }

  if (t === "assistant" && o.message && typeof o.message === "object") {
    const m = o.message as Record<string, unknown>;
    return {
      type: "assistant",
      message_id: String(m.id ?? ""),
      content: extractContent(m.content),
      usage: extractUsage(m.usage),
      raw: obj,
    };
  }

  if (t === "user" && o.message && typeof o.message === "object") {
    const m = o.message as Record<string, unknown>;
    return {
      type: "user",
      message_id: typeof m.id === "string" ? m.id : undefined,
      content: extractContent(m.content),
      raw: obj,
    };
  }

  if (t === "result") {
    return {
      type: "result",
      subtype: typeof o.subtype === "string" ? o.subtype : undefined,
      is_error: Boolean(o.is_error),
      duration_ms: typeof o.duration_ms === "number" ? o.duration_ms : undefined,
      total_cost_usd:
        typeof o.total_cost_usd === "number" ? o.total_cost_usd : undefined,
      result: typeof o.result === "string" ? o.result : undefined,
      stop_reason: typeof o.stop_reason === "string" ? o.stop_reason : undefined,
      raw: obj,
    };
  }

  if (t === "rate_limit_event") {
    return { type: "rate_limit", raw: obj };
  }

  return { type: "unknown", kind: t, raw: obj };
}

function extractContent(input: unknown): ClaudeContentBlock[] {
  if (!Array.isArray(input)) return [];
  return input.map((item): ClaudeContentBlock => {
    if (!item || typeof item !== "object") return { type: "unknown", raw: item };
    const i = item as Record<string, unknown>;
    const k = typeof i.type === "string" ? i.type : "";
    if (k === "text" && typeof i.text === "string") {
      return { type: "text", text: i.text };
    }
    if (k === "thinking" && typeof i.thinking === "string") {
      return {
        type: "thinking",
        thinking: i.thinking,
        signature: typeof i.signature === "string" ? i.signature : undefined,
      };
    }
    if (k === "tool_use" && typeof i.id === "string" && typeof i.name === "string") {
      return { type: "tool_use", id: i.id, name: i.name, input: i.input };
    }
    if (k === "tool_result" && typeof i.tool_use_id === "string") {
      return {
        type: "tool_result",
        tool_use_id: i.tool_use_id,
        content: i.content,
        is_error: typeof i.is_error === "boolean" ? i.is_error : undefined,
      };
    }
    return { type: "unknown", raw: item };
  });
}

function extractUsage(
  input: unknown,
): { input_tokens: number; output_tokens: number } | undefined {
  if (!input || typeof input !== "object") return undefined;
  const u = input as Record<string, unknown>;
  if (typeof u.input_tokens === "number" && typeof u.output_tokens === "number") {
    return { input_tokens: u.input_tokens, output_tokens: u.output_tokens };
  }
  return undefined;
}

// claude emits the same assistant message id multiple times (thinking, text,
// tool_use as separate stream events) — merge by id so the popup shows one row.
export function mergeAssistantMessages(
  events: ClaudeStreamEvent[],
): ClaudeStreamEvent[] {
  const merged: ClaudeStreamEvent[] = [];
  const indexById = new Map<string, number>();

  for (const ev of events) {
    if (ev.type === "assistant" && ev.message_id) {
      const existing = indexById.get(ev.message_id);
      if (existing !== undefined) {
        const prev = merged[existing];
        if (prev.type === "assistant") {
          // Append new content blocks that aren't already present (dedup by
          // serialized JSON to avoid double-rendering on stream replay).
          const seen = new Set(prev.content.map((b) => JSON.stringify(b)));
          for (const block of ev.content) {
            const key = JSON.stringify(block);
            if (!seen.has(key)) {
              prev.content.push(block);
              seen.add(key);
            }
          }
          if (ev.usage) prev.usage = ev.usage;
          continue;
        }
      }
      indexById.set(ev.message_id, merged.length);
    }
    merged.push(ev);
  }

  return merged;
}
