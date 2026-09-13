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
  | { type: "thinking"; thinking: string }
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

export type ToolInvocationTerminalState =
  | "success"
  | "failure"
  | "timeout"
  | "cancelled";

export interface ToolInvocationState {
  key: string;
  toolUseId: string;
  name: string;
  input: unknown;
  result?: string;
  isError: boolean;
  state: "running" | ToolInvocationTerminalState;
  terminalTransitionCount: number;
  firstSeenOrder: number;
  terminalSeenOrder?: number;
}

// Strip CSI/OSC/control bytes from ConPTY output.
// ConPTY can wrap long JSON in the middle of braces, so parse by brace balance, not newline.
const ANSI_PATTERN =
  /\x1b\[[0-9;?]*[a-zA-Z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|[\x00-\x08\x0b-\x1f\x7f]/g;

export class ClaudeStreamParser {
  private buffer = "";

  feed(chunk: string): ParseResult {
    this.buffer += chunk.replace(ANSI_PATTERN, "");
    const events: ClaudeStreamEvent[] = [];
    const rawLines: RawLine[] = [];

    let pos = 0;
    let unparsedStart = 0;

    while (pos < this.buffer.length) {
      const objStart = this.buffer.indexOf("{", pos);
      if (objStart < 0) {
        pos = this.buffer.length;
        break;
      }

      // Garbage between previous JSON and this `{` — surface as raw lines.
      if (objStart > unparsedStart) {
        const gap = this.buffer.slice(unparsedStart, objStart);
        for (const ln of gap.split(/\r?\n/)) {
          const t = ln.trim();
          if (t) rawLines.push({ text: ln });
        }
      }

      const objEnd = findBalancedEnd(this.buffer, objStart);
      if (objEnd < 0) {
        // Incomplete object — wait for more input.
        break;
      }

      const candidate = this.buffer.slice(objStart, objEnd + 1);
      try {
        const parsed = JSON.parse(candidate);
        const ev = mapEvent(parsed);
        if (ev) {
          events.push(ev);
        }
      } catch {
        // Looked balanced but invalid JSON — preserve as raw single line.
        rawLines.push({ text: candidate.slice(0, 300) });
      }
      pos = objEnd + 1;
      unparsedStart = pos;
    }

    this.buffer = this.buffer.slice(unparsedStart);
    return { events, rawLines };
  }

  reset() {
    this.buffer = "";
  }
}

function findBalancedEnd(s: string, start: number): number {
  let depth = 0;
  let inString = false;
  let escape = false;
  for (let i = start; i < s.length; i++) {
    const ch = s[i];
    if (escape) {
      escape = false;
      continue;
    }
    if (inString) {
      if (ch === "\\") {
        escape = true;
        continue;
      }
      if (ch === '"') inString = false;
      continue;
    }
    if (ch === '"') {
      inString = true;
      continue;
    }
    if (ch === "{") depth++;
    else if (ch === "}") {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
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

  // Drop SDK-internal partial-message deltas — the high-level assistant/user events already carry the full accumulated content.
  if (t === "stream_event" || t === "message" || t === "direct") {
    return null;
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
      // Drop signature — opaque base64 (~700-1500 chars per block) only used for chaining multi-turn API requests; one-shot workers don't need it.
      return { type: "thinking", thinking: i.thinking };
    }
    if (k === "redacted_thinking") {
      return { type: "thinking", thinking: "[redacted thinking]" };
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

// Claude can emit one assistant message id across multiple stream chunks.
// Merge by id to show one combined popup row.
export function mergeAssistantMessages(
  events: ClaudeStreamEvent[],
): ClaudeStreamEvent[] {
  const merged: ClaudeStreamEvent[] = [];
  const indexById = new Map<string, number>();

  for (const ev of events) {
    if ((ev.type === "assistant" || ev.type === "user") && ev.message_id) {
      const existing = indexById.get(ev.message_id);
      if (existing !== undefined) {
        const prev = merged[existing];
        if (
          (prev.type === "assistant" || prev.type === "user")
          && prev.type === ev.type
        ) {
          // Append new content blocks that aren't already present (dedup by serialized JSON to avoid double-rendering on stream replay).
          const seen = new Set(prev.content.map((b) => JSON.stringify(b)));
          for (const block of ev.content) {
            const key = JSON.stringify(block);
            if (!seen.has(key)) {
              prev.content.push(block);
              seen.add(key);
            }
          }
          if (ev.type === "assistant" && prev.type === "assistant" && ev.usage) {
            prev.usage = ev.usage;
          }
          continue;
        }
      }
      indexById.set(ev.message_id, merged.length);
    }
    merged.push(ev);
  }

  return merged;
}

function invocationKey(toolUseId: string): string {
  return `tool:${toolUseId}`;
}

function stringifyToolResult(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .map((item) => {
        if (
          item
          && typeof item === "object"
          && "text" in item
          && typeof (item as { text: unknown }).text === "string"
        ) {
          return (item as { text: string }).text;
        }
        return JSON.stringify(item);
      })
      .join("\n");
  }
  if (content && typeof content === "object") return JSON.stringify(content, null, 2);
  return String(content ?? "");
}

function classifyTerminalState(
  result: string,
  isError: boolean,
): ToolInvocationTerminalState {
  if (!isError) return "success";
  const lc = result.toLowerCase();
  if (
    lc.includes("timed out")
    || lc.includes("timeout")
    || lc.includes("deadline exceeded")
  ) {
    return "timeout";
  }
  if (
    lc.includes("cancelled")
    || lc.includes("canceled")
    || lc.includes("interrupted")
    || lc.includes("aborted")
  ) {
    return "cancelled";
  }
  return "failure";
}

export function reconcileToolInvocations(
  events: ClaudeStreamEvent[],
): ToolInvocationState[] {
  const merged = mergeAssistantMessages(events);
  const invocations = new Map<string, ToolInvocationState>();
  let order = 0;

  const ensureInvocation = (
    toolUseId: string,
    patch: Partial<ToolInvocationState>,
  ): ToolInvocationState => {
    const key = invocationKey(toolUseId);
    const existing = invocations.get(key);
    if (existing) {
      Object.assign(existing, patch);
      return existing;
    }
    const created: ToolInvocationState = {
      key,
      toolUseId,
      name: "",
      input: null,
      isError: false,
      state: "running",
      terminalTransitionCount: 0,
      firstSeenOrder: order++,
      ...patch,
    };
    invocations.set(key, created);
    return created;
  };

  for (const event of merged) {
    if (event.type !== "assistant" && event.type !== "user") continue;
    for (const block of event.content) {
      if (block.type === "tool_use") {
        ensureInvocation(block.id, {
          name: block.name,
          input: block.input,
        });
        continue;
      }
      if (block.type !== "tool_result") continue;
      const result = stringifyToolResult(block.content);
      const nextState = classifyTerminalState(result, block.is_error === true);
      const invocation = ensureInvocation(block.tool_use_id, {});
      if (invocation.state === "running") {
        invocation.state = nextState;
        invocation.result = result;
        invocation.isError = block.is_error === true;
        invocation.terminalTransitionCount = 1;
        invocation.terminalSeenOrder = order++;
        continue;
      }
    }
  }

  return [...invocations.values()].sort((a, b) => a.firstSeenOrder - b.firstSeenOrder);
}
