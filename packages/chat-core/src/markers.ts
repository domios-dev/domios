/**
 * Unified CodeTerm message markers — TS twin of
 * `src-tauri/src/message_markers.rs`. Keep the two in lock-step (the parity
 * fixture lives in both test sets).
 *
 *   -=-codeterm:<kind> k=v k=v …-=-<payload>
 *
 * The `codeterm:` sentinel makes accidental reproduction unlikely. Attribute
 * values percent-encode the structural chars so payloads/labels never break
 * the frame.
 */
export const MARKER_OPEN = "-=-codeterm:";
export const MARKER_CLOSE = "-=-";
const BRACKETED_PASTE_START = "\x1b[200~";
const BRACKETED_PASTE_END = "\x1b[201~";

function encode(value: string): string {
  let out = "";
  for (const ch of value) {
    switch (ch) {
      case "%": out += "%25"; break;
      case " ": out += "%20"; break;
      case "=": out += "%3D"; break;
      case "\n": out += "%0A"; break;
      case "\t": out += "%09"; break;
      case '"': out += "%22"; break;
      default: out += ch;
    }
  }
  return out;
}

function decode(value: string): string {
  return value.replace(/%([0-9A-Fa-f]{2})/g, (_, h) => String.fromCharCode(parseInt(h, 16)));
}

export interface ParsedMarker {
  kind: string;
  attrs: Record<string, string>;
  payload: string;
}

export function isTabMarkerPlaceholderLabel(label: string | null | undefined): boolean {
  const normalized = label?.trim().toLowerCase();
  return !normalized || normalized === "current tab";
}

export function shortTabRef(tabRef: string | null | undefined): string {
  return (tabRef ?? "").trim().slice(0, 8);
}

export function buildMarker(
  kind: string,
  attrs: Array<[string, string]> = [],
  payload = "",
): string {
  let head = MARKER_OPEN + kind;
  for (const [k, v] of attrs) head += ` ${k}=${encode(v)}`;
  return head + MARKER_CLOSE + payload;
}

/** Parse a marker, or null if `text` isn't a CodeTerm marker frame. */
export function parseMarker(text: string): ParsedMarker | null {
  let normalized = text.replace(/^\s+/, "");
  if (normalized.startsWith(BRACKETED_PASTE_START)) {
    normalized = normalized.slice(BRACKETED_PASTE_START.length);
    if (normalized.endsWith(BRACKETED_PASTE_END)) {
      normalized = normalized.slice(0, -BRACKETED_PASTE_END.length);
    }
  }
  if (!normalized.startsWith(MARKER_OPEN)) return null;
  const rest = normalized.slice(MARKER_OPEN.length);
  const close = rest.indexOf(MARKER_CLOSE);
  if (close < 0) return null;
  const head = rest.slice(0, close);
  const payload = rest.slice(close + MARKER_CLOSE.length);
  const tokens = head.split(" ");
  const kind = tokens.shift() ?? "";
  if (!kind) return null;
  const attrs: Record<string, string> = {};
  for (const tok of tokens) {
    if (!tok) continue;
    const eq = tok.indexOf("=");
    if (eq > 0) attrs[tok.slice(0, eq)] = decode(tok.slice(eq + 1));
  }
  return { kind, attrs, payload };
}

const opt = (k: string, v: string | null | undefined): Array<[string, string]> =>
  v ? [[k, v]] : [];

export const markVoice = (file: string, transcript?: string | null, note?: string | null) =>
  transcript
    ? buildMarker("voice", [["file", file]], transcript)
    : note
      ? buildMarker("voice", [["file", file], ["status", "untranscribed"]], note)
      : buildMarker("voice", [["file", file]]);
export const markPhoto = (file: string, name?: string | null) =>
  buildMarker("photo", [["file", file], ...opt("name", name)]);
export const markAttachment = (file: string, name?: string | null) =>
  buildMarker("file", [["file", file], ...opt("name", name)]);
export const markTelegram = (user: string, msg: string | null, body: string) =>
  buildMarker("telegram", [["user", user], ...opt("msg", msg)], body);
export const markMesh = (peer: string, tab: string, body: string) =>
  buildMarker("mesh", [["peer", peer], ["tab", tab]], body);
export const markTab = (label: string, tab: string, body: string) =>
  buildMarker("tab", [["label", label], ["tab", tab]], body);
export const markPluginConfig = (id: string, name: string, body: string): string =>
  buildMarker("plugin_config", [["plugin", id], ["name", name]], body);
export const markReport = (from: string, body: string) =>
  buildMarker("report", [["from", from]], body);
export const markHook = (json: string) => buildMarker("hook", [], json);
export const markSystemPrompt = (body: string) => buildMarker("system_prompt", [], body);
export const markContextRequest = (body: string) => buildMarker("context_request", [], body);
export const markWatcherVerdict = (body: string) => buildMarker("watcher_verdict", [], body);
