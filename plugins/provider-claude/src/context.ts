export interface TurnContext {
  used_tokens: number;
  window_tokens: number;
  cost_usd: number | null;
  source: string;
}

const LIMIT = 10_000_000;
function count(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 && value <= LIMIT ? value : null;
}

export function parseContextUsage(record: any): TurnContext | null {
  if (!record || typeof record !== "object") return null;
  const statusline = record.context_window;
  const hasStatusline = Object.prototype.hasOwnProperty.call(record, "context_window");
  const usage = hasStatusline ? statusline?.current_usage : record.type === "assistant" ? record.message?.usage : null;
  if (!usage || typeof usage !== "object") return null;
  const input = count(usage.input_tokens);
  const creation = usage.cache_creation_input_tokens === undefined ? 0 : count(usage.cache_creation_input_tokens);
  const read = usage.cache_read_input_tokens === undefined ? 0 : count(usage.cache_read_input_tokens);
  if (input === null || creation === null || read === null || input + creation + read > LIMIT) return null;
  const window = count(statusline?.context_window_size !== undefined ? statusline.context_window_size
    : usage.context_window_size !== undefined ? usage.context_window_size : 0);
  if (window === null) return null;
  const rawCost = record.cost?.total_cost_usd;
  const cost = typeof rawCost === "number" && Number.isFinite(rawCost) && rawCost >= 0 && rawCost <= 1_000_000 ? rawCost : null;
  return { used_tokens: input + creation + read, window_tokens: window, cost_usd: cost,
    source: hasStatusline ? "claude-statusline" : "claude-jsonl" };
}

export const STATUSLINE_FLAG = "--codeterm-statusline";
export const STATUSLINE_FORWARD = 'payload=$(cat); if [ -n "${CODETERM_API_URL:-}" ] && [ -n "${CODETERM_API_TOKEN:-}" ] && [ -n "${CODETERM_TAB_ID:-}" ]; then printf \'%s\' "$payload" | curl -s --max-time 2 -o /dev/null -X POST "$CODETERM_API_URL/api/tabs/$CODETERM_TAB_ID/hook/context" -H \'Content-Type: application/json\' -H "Authorization: Bearer $CODETERM_API_TOKEN" -H "X-CodeTerm-Binding-Nonce: ${CODETERM_SESSION_BINDING_NONCE:-}" --data-binary @- || true; fi';

export function statuslineSettings(settings: unknown): string | null {
  if (!settings || typeof settings !== "object" || Array.isArray(settings)) return null;
  const line = (settings as any).statusLine;
  if (line !== undefined && (!line || line.type !== "command" || typeof line.command !== "string" || !line.command.trim())) return null;
  if (line) {
    let bytes = 0;
    for (const character of line.command as string) {
      const point = character.codePointAt(0)!;
      bytes += point > 0xffff ? 4 : point > 0x7ff ? 3 : point > 0x7f ? 2 : 1;
    }
    if (bytes > 8192) return null;
    if (line.padding !== undefined && (count(line.padding) === null || line.padding > 1000)) return null;
  }
  const command = STATUSLINE_FORWARD + (line ? `; printf '%s' "$payload" | ( ${line.command}\n)` : "; true");
  return JSON.stringify({ statusLine: { command, ...(line?.padding !== undefined ? { padding: line.padding } : {}), type: "command" } });
}

export function statuslineLaunchArgs(args: string[]): string[] {
  const enabled = args.includes(STATUSLINE_FLAG);
  const rest = args.filter(arg => arg !== STATUSLINE_FLAG);
  if (!enabled || rest.some(arg => arg === "--settings" || arg.startsWith("--settings="))) return rest;
  const path = host.expandHome("~/.claude/settings.json");
  if (!path) return rest;
  const text = host.fs.fileExists(path) ? host.fs.readFileHead(path, 65_537) : "{}";
  if (text === null || text.length > 65_536) return rest;
  try {
    const settings = statuslineSettings(JSON.parse(text));
    return settings ? [...rest, "--settings", settings] : rest;
  } catch { return rest; }
}
