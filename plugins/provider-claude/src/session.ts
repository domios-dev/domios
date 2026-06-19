// Session JSONL types, validators, file-path resolution, and model-label
// helpers — the shared substrate behind detectSessionModel / parseSessionMessages
// / parseSessionDelta / sessionJsonlPath in the provider facade.
import { manifest } from "./shared";

export interface ClaudeContentBlock { type?: string; text?: string; [k: string]: unknown }
export interface ClaudeMessage { content?: string | ClaudeContentBlock[]; model?: string }
export interface ClaudeEntry {
  type?: string;
  subtype?: string;
  content?: string | ClaudeContentBlock[];
  message?: ClaudeMessage;
  attachment?: { type?: string; prompt?: string };
  timestamp?: string;
  uuid?: string;
  isMeta?: boolean;
}

const CLAUDE_SESSION_DELTA_TYPES: Record<string, boolean> = { system: true, user: true, assistant: true, attachment: true };

// Session-file/model-label helpers aligned with native `~/.claude/projects`.
// `encode_cwd_dashed` maps path separators to `-` and matches JsProvider path rules.
export function claudeSessionFilePath(cwd: string, sessionId: string): string | null {
  const m = manifest();
  const root = (m && m.sessions && m.sessions.rootDir) || "~/.claude/projects";
  const ext = (m && m.sessions && m.sessions.fileExtension) || "jsonl";
  const expanded = host.expandHome(root);
  if (!expanded) return null;
  const encoded = encodeCwdDashed(cwd);
  const sep = host.platform() === "windows" ? "\\" : "/";
  return expanded + sep + encoded + sep + sessionId + "." + ext;
}

function encodeCwdDashed(cwd: string): string {
  // Match native behavior: map non-alphanumeric characters to '-'.
  return String(cwd || "").replace(/[^a-zA-Z0-9]/g, "-");
}

export function contentToText(c: string | ClaudeContentBlock[] | undefined): string {
  if (typeof c === "string") return c;
  if (!Array.isArray(c)) return "";
  const chunks: string[] = [];
  for (let i = 0; i < c.length; i++) {
    const b = c[i];
    if (b && typeof b.text === "string") chunks.push(b.text);
  }
  return chunks.join(" ");
}

export function isValidClaudeSessionMessage(v: ClaudeEntry): boolean {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (v.type !== "user" && v.type !== "assistant") return false;
  if (!v.message || typeof v.message !== "object" || Array.isArray(v.message)) return false;
  const content = v.message.content;
  if (typeof content !== "string" && !Array.isArray(content)) return false;
  return true;
}

export function validateEntry(v: ClaudeEntry): boolean {
  return isValidClaudeSessionDelta(v);
}

function isValidClaudeSessionDelta(v: ClaudeEntry): boolean {
  if (!v || typeof v !== "object" || Array.isArray(v)) return false;
  if (!CLAUDE_SESSION_DELTA_TYPES[v.type as string]) return false;
  if (v.type === "system") {
    const sc = v.content;
    return typeof sc === "string" || Array.isArray(sc);
  }
  if (v.type === "attachment") {
    const a = v.attachment;
    return !!a && typeof a === "object" && a.type === "queued_command" && typeof a.prompt === "string" && a.prompt.length > 0;
  }
  const msg = v.message;
  if (!msg || typeof msg !== "object" || Array.isArray(msg)) return false;
  if (typeof msg.content !== "string" && !Array.isArray(msg.content)) return false;
  return true;
}

export function extractSetModelFamily(raw: string): string | null {
  const idx = raw.indexOf("Set model to ");
  if (idx === -1) return null;
  const after = raw.substring(idx + "Set model to ".length);
  const cleaned = stripAnsi(after).replace(/^\s+/, "");
  let fam = "";
  for (let i = 0; i < cleaned.length; i++) {
    const c = cleaned.charAt(i);
    if (!/[a-zA-Z]/.test(c)) break;
    fam += c;
  }
  return fam ? capitalize(fam) : null;
}

function stripAnsi(s: string): string {
  let out = "";
  let i = 0;
  while (i < s.length) {
    if (s.charCodeAt(i) === 0x1b && s.charAt(i + 1) === "[") {
      i += 2;
      while (i < s.length && !/[a-zA-Z]/.test(s.charAt(i))) i++;
      if (i < s.length) i++;
    } else {
      out += s.charAt(i);
      i++;
    }
  }
  return out;
}

export function familyFromModelId(id: string): string {
  const m = String(id || "").toLowerCase();
  const fams = ["opus", "sonnet", "haiku"];
  for (let i = 0; i < fams.length; i++) {
    if (m.indexOf(fams[i]) !== -1) return capitalize(fams[i]);
  }
  const body = m.indexOf("claude-") === 0 ? m.substring(7) : m;
  const head = body.split("-")[0] || body;
  return capitalize(head);
}

function capitalize(s: string): string {
  if (!s) return "";
  return s.charAt(0).toUpperCase() + s.substring(1).toLowerCase();
}
