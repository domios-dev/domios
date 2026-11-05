const MUX_STREAM_PATHS = new Set(["/api/mesh/relay-stream", "/api/output/stream", "/api/events/stream"]);

/** Startup readiness stays a direct, finite stream: the carrier's own session waits for that readiness. */
export function isDirectDaemonPath(path: string): boolean {
  return new URL(path, "http://daemon").pathname === "/api/startup/events";
}

export function isMuxStreamPath(path: string): boolean {
  if (!path.startsWith("/") || path.startsWith("//") || path.includes("\\")) return false;
  const pathname = new URL(path, "http://daemon").pathname;
  return MUX_STREAM_PATHS.has(pathname)
    || /^\/api\/tabs\/[^/]+\/output\/stream$/.test(pathname)
    || /^\/api\/surfaces\/[^/]+\/stream$/.test(pathname);
}

/** Cumulative credited bytes are posted every 256 KiB, a sixteenth of the daemon's 4 MiB bulk window. */
export const BULK_CREDIT_STEP = 256 * 1024;
/** A lost cumulative credit is re-posted this many times before the session is left to its own loss handling. */
export const CREDIT_ATTEMPTS = 3;

export function bulkCreditDue(received: number, posted: number): number | null {
  return received - posted >= BULK_CREDIT_STEP ? received : null;
}

/** A refused mux session is an authentication answer, not a lost connection: never retry it. */
export function muxSessionRefused(status: number | undefined): boolean {
  return status === 401 || status === 403;
}

export type MuxChannelEnd =
  | { reason: "eof" }
  | { reason: "error"; message: string }
  | { reason: "capacity"; bytes: number };

export type MuxFrame =
  | { channel: string; kind: "open"; status: number; headers: [string, string][] }
  | { channel: string; kind: "chunk"; chunk: string; enc: "utf8" | "base64"; credit: number }
  | { channel: string; kind: "end"; end: MuxChannelEnd };

function channelEnd(value: unknown): MuxChannelEnd | null {
  if (typeof value !== "object" || value === null || !("reason" in value)) return null;
  const end = value as Record<string, unknown>;
  switch (end.reason) {
    case "eof": return { reason: "eof" };
    case "error": return { reason: "error", message: String(end.message ?? "") };
    case "capacity": return typeof end.bytes === "number" ? { reason: "capacity", bytes: end.bytes } : null;
    default: return null;
  }
}

export function decodeMuxFrame(value: unknown): MuxFrame | null {
  if (typeof value !== "object" || value === null) return null;
  const frame = value as Record<string, unknown>;
  if (typeof frame.channel !== "string" || !frame.channel) return null;
  const { channel } = frame;
  if ("end" in frame) {
    const end = channelEnd(frame.end);
    return end ? { channel, kind: "end", end } : null;
  }
  if (frame.open === true && Number.isInteger(frame.status) && Number(frame.status) >= 200 && Number(frame.status) <= 599
    && Array.isArray(frame.headers) && frame.headers.every(pair => Array.isArray(pair) && pair.length === 2 && pair.every(value => typeof value === "string"))) {
    return { channel, kind: "open", status: Number(frame.status), headers: frame.headers as [string, string][] };
  }
  if (typeof frame.chunk === "string" && (frame.enc === "utf8" || frame.enc === "base64")) {
    return { channel, kind: "chunk", chunk: frame.chunk, enc: frame.enc, credit: Number.isSafeInteger(frame.credit) && Number(frame.credit) > 0 ? Number(frame.credit) : 0 };
  }
  return null;
}
