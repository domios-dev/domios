import { MAX_COPY_TEXT } from "../../lib/pluginViewHostActions";

/** Matches the local host clipboard write ceiling (`MAX_COPY_TEXT`). */
export const MAX_OSC52_TEXT_BYTES = MAX_COPY_TEXT;

/** Intent window bridging user keypress/click to a remote PTY OSC 52 reply over mesh RTT. */
export const OSC52_USER_INTENT_WINDOW_MS = 5_000;

const MAX_OSC52_BASE64_CHARS = Math.ceil((MAX_OSC52_TEXT_BYTES * 4) / 3) + 4;
const VALID_SELECTION_RE = /^[cpqs0-7]*$/;
const VALID_BASE64_RE = /^[A-Za-z0-9+/]*={0,2}$/;

export interface Osc52Context {
  isRemoteTab: boolean;
  terminalActive: boolean;
  replayingHistory: boolean;
  nowMs: number;
  lastUserIntentAtMs: number | null;
  intentWindowMs?: number;
}

export type Osc52RejectReason =
  | "malformed"
  | "read_query_forbidden"
  | "unsupported_selection"
  | "empty"
  | "too_large"
  | "inactive_terminal"
  | "replaying_history"
  | "missing_user_intent"
  | "invalid_base64";

export type Osc52Decision =
  | { action: "write"; text: string; target: "viewer_clipboard" }
  | { action: "ignore"; reason: Osc52RejectReason };

function decodeBase64Utf8(clean: string): { ok: true; bytes: Uint8Array; text: string } | { ok: false } {
  if (clean.length % 4 === 1 || !VALID_BASE64_RE.test(clean)) return { ok: false };
  const padded = clean.length % 4 === 0 ? clean : clean.padEnd(clean.length + (4 - (clean.length % 4)), "=");
  try {
    const binary = atob(padded);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i += 1) {
      bytes[i] = binary.charCodeAt(i);
    }
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return { ok: true, bytes, text };
  } catch {
    return { ok: false };
  }
}

/**
 * Pure routing and safety gate for OSC 52 (`ESC ] 52 ; Pc ; Pd ST`).
 *
 * - Read queries (`?`) are always refused so viewer clipboard contents can never
 *   be exfiltrated to a local or remote PTY.
 * - Clipboard writes always target the viewing client's clipboard and require
 *   an active, non-replaying terminal with recent user intent inside the window.
 * - Size limits match the local clipboard ceiling (`MAX_OSC52_TEXT_BYTES`).
 */
export function evaluateOsc52ClipboardWrite(
  rawData: string,
  context: Osc52Context,
): Osc52Decision {
  const sep = rawData.indexOf(";");
  if (sep < 0) return { action: "ignore", reason: "malformed" };

  const selection = rawData.slice(0, sep);
  const rawPayload = rawData.slice(sep + 1);
  if (rawPayload.trim() === "?") {
    return { action: "ignore", reason: "read_query_forbidden" };
  }

  if (!VALID_SELECTION_RE.test(selection)) {
    return { action: "ignore", reason: "unsupported_selection" };
  }
  if (selection !== "" && !selection.includes("c")) {
    return { action: "ignore", reason: "unsupported_selection" };
  }

  const cleanBase64 = rawPayload.replace(/\s+/g, "");
  if (!cleanBase64) return { action: "ignore", reason: "empty" };
  if (cleanBase64.length > MAX_OSC52_BASE64_CHARS) {
    return { action: "ignore", reason: "too_large" };
  }

  if (!context.terminalActive) {
    return { action: "ignore", reason: "inactive_terminal" };
  }
  if (context.replayingHistory) {
    return { action: "ignore", reason: "replaying_history" };
  }

  const intentWindowMs = context.intentWindowMs ?? OSC52_USER_INTENT_WINDOW_MS;
  const elapsed = context.lastUserIntentAtMs === null ? Infinity : context.nowMs - context.lastUserIntentAtMs;
  if (elapsed < 0 || elapsed > intentWindowMs) {
    return { action: "ignore", reason: "missing_user_intent" };
  }

  const decoded = decodeBase64Utf8(cleanBase64);
  if (!decoded.ok) return { action: "ignore", reason: "invalid_base64" };
  if (decoded.bytes.length > MAX_OSC52_TEXT_BYTES || decoded.text.length > MAX_OSC52_TEXT_BYTES) {
    return { action: "ignore", reason: "too_large" };
  }
  if (!decoded.text) return { action: "ignore", reason: "empty" };

  return { action: "write", text: decoded.text, target: "viewer_clipboard" };
}
