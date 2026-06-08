/**
 * Unified CodeTerm message markers — TS twin of
 * `src-tauri/src/message_markers.rs`. Keep the two in lock-step (the parity
 * fixture lives in both test sets).
 *
 *   [[ct:<kind> k=v k=v …]]<payload>
 *
 * The `ct:` namespace makes accidental reproduction unlikely; the sentinel is
 * the OPEN/CLOSE constants so a stricter form is a one-line change. Attribute
 * values percent-encode the structural chars so payloads/labels never break
 * the frame.
 */
export const MARKER_NS = "ct";
export const MARKER_OPEN = "[[ct:";
export const MARKER_CLOSE = "]]";

function encode(value: string): string {
  let out = "";
  for (const ch of value) {
    switch (ch) {
      case "%": out += "%25"; break;
      case " ": out += "%20"; break;
      case "]": out += "%5D"; break;
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

export function buildMarker(
  kind: string,
  attrs: Array<[string, string]> = [],
  payload = "",
): string {
  let head = MARKER_OPEN + kind;
  for (const [k, v] of attrs) head += ` ${k}=${encode(v)}`;
  return head + MARKER_CLOSE + payload;
}

/** Parse a marker, or null if `text` isn't a `[[ct:…]]` frame. */
export function parseMarker(text: string): ParsedMarker | null {
  if (!text.startsWith(MARKER_OPEN)) return null;
  const rest = text.slice(MARKER_OPEN.length);
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

// ── Convenience constructors (mirror the Rust ones) ──
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
export const markMesh = (peer: string, pane: string, body: string) =>
  buildMarker("mesh", [["peer", peer], ["pane", pane]], body);
export const markPane = (label: string, pane: string, body: string) =>
  buildMarker("pane", [["label", label], ["pane", pane]], body);
export const markReport = (from: string, body: string) =>
  buildMarker("report", [["from", from]], body);
export const markHook = (json: string) => buildMarker("hook", [], json);
