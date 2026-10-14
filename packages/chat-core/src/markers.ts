import { attachmentMarkerSegments, formatAttachmentMarker } from "./attachmentMarker";
import { buildDomiosMessage, parseDomiosMessage } from "./domios";

/**
 * Legacy non-routing markers — TS twin of `src-tauri/src/message_markers.rs`.
 * Routed messages use the XML-tagged Domios frame parser from `domios.ts`.
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
  // Newline after the frame so the body reads apart from the routing
  // header; parseMarker strips it so the payload is unchanged.
  return head + MARKER_CLOSE + (payload ? "\n" + payload : "");
}

/** Parse a marker, or null if `text` isn't a CodeTerm marker frame. */
export function parseMarker(text: string): ParsedMarker | null {
  const domios = parseDomiosMessage(text);
  if (domios) return { kind: domios.attrs.from, attrs: domios.attrs, payload: domios.body };
  let normalized = text.replace(/^\s+/, "");
  if (normalized.startsWith(BRACKETED_PASTE_START)) {
    normalized = normalized.slice(BRACKETED_PASTE_START.length);
    if (normalized.endsWith(BRACKETED_PASTE_END)) {
      normalized = normalized.slice(0, -BRACKETED_PASTE_END.length);
    }
  }
  if (normalized.startsWith("<domios ")) {
    const end = normalized.indexOf("/>");
    if (end < 0) return null;
    const segments = attachmentMarkerSegments(normalized);
    const first = segments[0];
    if (!first || !("attachment" in first) || first.attachment.kind !== "voice") return null;
    const head = normalized.slice(0, end + 2);
    if (/\sfrom=/.test(head)) return null;
    const attrs: Record<string, string> = { file: first.attachment.path };
    if (/\sstatus="untranscribed"/.test(head)) attrs.status = "untranscribed";
    const body = normalized.slice(head.length);
    return { kind: "voice", attrs, payload: voicePayload(body.replace(/^(?:\r\n|[\r\n])/, "")) };
  }
  if (!normalized.startsWith(MARKER_OPEN)) return null;
  const rest = normalized.slice(MARKER_OPEN.length);
  const close = rest.indexOf(MARKER_CLOSE);
  if (close < 0) return null;
  const head = rest.slice(0, close);
  const raw = rest.slice(close + MARKER_CLOSE.length);
  // A PTY rewrites the separator to CR in transit, so accept either form.
  const payload = raw.startsWith("\r\n")
    ? raw.slice(2)
    : raw.startsWith("\n") || raw.startsWith("\r")
      ? raw.slice(1)
      : raw;
  const tokens = head.split(" ");
  const kind = tokens.shift() ?? "";
  if (!kind || isLegacyRouteKind(kind)) return null;
  const attrs: Record<string, string> = {};
  for (const tok of tokens) {
    if (!tok) continue;
    const eq = tok.indexOf("=");
    if (eq > 0) attrs[tok.slice(0, eq)] = decode(tok.slice(eq + 1));
  }
  return { kind, attrs, payload };
}

function isLegacyRouteKind(kind: string): boolean {
  return ["tab", "report", "system", "reminder", "mesh", "telegram", "pebble", "inbound"].includes(kind);
}

const opt = (k: string, v: string | null | undefined): Array<[string, string]> =>
  v ? [[k, v]] : [];

const TRANSCRIPT_OPEN = "<transcript>";
const TRANSCRIPT_CLOSE = "</transcript>";

function escapeTranscript(value: string): string {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function unescapeTranscript(value: string): string {
  let decoded = "";
  let index = 0;
  while (index < value.length) {
    if (value.startsWith("&lt;", index)) {
      decoded += "<";
      index += 4;
    } else if (value.startsWith("&gt;", index)) {
      decoded += ">";
      index += 4;
    } else if (value.startsWith("&amp;", index)) {
      decoded += "&";
      index += 5;
    } else {
      decoded += value[index];
      index += 1;
    }
  }
  return decoded;
}

function voicePayload(payload: string): string {
  if (!payload.startsWith(TRANSCRIPT_OPEN)) return payload;
  const end = payload.indexOf(TRANSCRIPT_CLOSE);
  if (end < 0) return payload;
  const tail = payload.slice(end + TRANSCRIPT_CLOSE.length);
  if (tail.trim()) return payload;
  return unescapeTranscript(payload.slice(TRANSCRIPT_OPEN.length, end));
}

/** A transcript is one element. An untranscribed note keeps `status` on the frame and no element. */
export function markVoice(file: string, transcript?: string | null, note?: string | null): string {
  const marker = formatAttachmentMarker(file, "voice", "voice");
  const untranscribed = transcript == null && note != null;
  const head = untranscribed ? marker.replace(" />", ' status="untranscribed" />') : marker;
  if (transcript) return `${head}\n${TRANSCRIPT_OPEN}${escapeTranscript(transcript)}${TRANSCRIPT_CLOSE}`;
  return note ? `${head}\n${note}` : head;
}
export const markPhoto = (file: string, name?: string | null) =>
  buildMarker("photo", [["file", file], ...opt("name", name)]);
export const markAttachment = (file: string, name?: string | null) =>
  buildMarker("file", [["file", file], ...opt("name", name)]);
export const markTelegram = (user: string, msg: string | null, body: string) =>
  buildDomiosMessage({ from: "telegram", user, ...(msg ? { msg } : {}) }, body);
export const markMesh = (peer: string, tab: string, body: string) =>
  buildDomiosMessage({ from: "mesh", peer, tab }, body);
export const markTab = (label: string, tab: string, body: string) =>
  buildDomiosMessage({ from: "tab", label, tab }, body);
export const markPluginConfig = (id: string, name: string, body: string): string =>
  buildMarker("plugin_config", [["plugin", id], ["name", name]], body);
export const markReport = (from: string, body: string) =>
  buildDomiosMessage({ from: "report", label: from }, body);
export const markHook = (json: string) => buildMarker("hook", [], json);
export const markSystemPrompt = (body: string) => buildMarker("system_prompt", [], body);
export const markContextRequest = (body: string) => buildMarker("context_request", [], body);
export const markWatcherVerdict = (body: string) => buildMarker("watcher_verdict", [], body);
