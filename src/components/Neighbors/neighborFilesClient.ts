import { listen } from "../../lib/bridge";
import { localFetch } from "../../lib/webApi";
import { receipt, type NeighborReceipt } from "./neighborRuntimeClient";

export type NeighborFileState = "saved" | "failed" | "expired" | "downloading";

export interface NeighborFile {
  file_id: string;
  name: string;
  size_bytes: number;
  state: NeighborFileState;
  message_id: string;
  direction: "in" | "out";
}

export interface NeighborFileQuota {
  friend: { limit_bytes: number; used_bytes: number; resets_at: number };
  sender: { limit_bytes: number; used_bytes: number; limit_files: number; used_files: number; resets_at: number };
}

/** A refused files call keeps the daemon's reason and limit, so the composer can say what was too much. */
export class NeighborFilesFailure extends Error {
  constructor(readonly reason: string, message: string, readonly limitBytes: number | null) { super(message); }
}

export interface NeighborFilesClient {
  send(friendship: string, body: string, paths: string[], inReplyTo?: string | null): Promise<NeighborReceipt>;
  list(friendship: string, signal?: AbortSignal): Promise<{ folder: string; files: NeighborFile[] }>;
  open(friendship: string, fileId: string): Promise<void>;
  openFolder(friendship: string): Promise<void>;
  setDailyLimit(friendship: string, bytes: number | null): Promise<void>;
  usage(friendship: string, signal?: AbortSignal): Promise<NeighborFileQuota | null>;
  onFilesChanged(handler: (friendship: string) => void): Promise<() => void>;
}

const STATES: readonly string[] = ["saved", "failed", "expired", "downloading"];
const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null;
const count = (value: unknown): value is number => typeof value === "number" && Number.isFinite(value) && value >= 0;

async function call(command: Record<string, unknown>, signal?: AbortSignal): Promise<unknown> {
  const response = await localFetch("/api/account/neighbors/files", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(command), signal,
  });
  if (!response.ok) {
    const failure: unknown = await response.json().catch(() => null);
    const reason = record(failure) && typeof failure.reason === "string" ? failure.reason : record(failure) && typeof failure.code === "string" ? failure.code : "unavailable";
    const message = record(failure) && typeof failure.message === "string" ? failure.message : `Files request failed (${response.status})`;
    throw new NeighborFilesFailure(reason, message, record(failure) && count(failure.limit_bytes) ? failure.limit_bytes : null);
  }
  return response.json();
}

export function parseFile(value: unknown): NeighborFile | null {
  if (!record(value) || typeof value.file_id !== "string" || typeof value.name !== "string" || !count(value.size_bytes)
    || !STATES.includes(String(value.state)) || typeof value.message_id !== "string" || (value.direction !== "in" && value.direction !== "out")) return null;
  return { file_id: value.file_id, name: value.name, size_bytes: value.size_bytes, state: value.state as NeighborFileState,
    message_id: value.message_id, direction: value.direction };
}

function parseQuota(value: unknown): NeighborFileQuota | null {
  if (!record(value) || !record(value.friend) || !record(value.sender)) return null;
  const { friend, sender } = value;
  if (![friend.limit_bytes, friend.used_bytes, friend.resets_at, sender.limit_bytes, sender.used_bytes, sender.limit_files, sender.used_files, sender.resets_at].every(count)) return null;
  // Reset times may come in seconds or milliseconds; the UI keeps milliseconds.
  const millis = (at: unknown) => (at as number) < 1e12 ? (at as number) * 1000 : at as number;
  return {
    friend: { limit_bytes: friend.limit_bytes as number, used_bytes: friend.used_bytes as number, resets_at: millis(friend.resets_at) },
    sender: { limit_bytes: sender.limit_bytes as number, used_bytes: sender.used_bytes as number, limit_files: sender.limit_files as number,
      used_files: sender.used_files as number, resets_at: millis(sender.resets_at) },
  };
}

export const neighborFilesClient: NeighborFilesClient = {
  send: async (friendship, body, paths, inReplyTo = null) =>
    receipt(await call({ operation: "send", friendship_id: friendship, kind: "question", body, in_reply_to: inReplyTo, paths })),
  list: async (friendship, signal) => {
    const value = await call({ operation: "list", friendship_id: friendship }, signal);
    if (!record(value) || typeof value.folder !== "string" || !Array.isArray(value.files)) throw new Error("Invalid Neighbor files");
    return { folder: value.folder, files: value.files.flatMap(file => { const parsed = parseFile(file); return parsed ? [parsed] : []; }) };
  },
  open: async (friendship, fileId) => { await call({ operation: "open", friendship_id: friendship, file_id: fileId }); },
  openFolder: async friendship => { await call({ operation: "open_folder", friendship_id: friendship }); },
  setDailyLimit: async (friendship, bytes) => { await call({ operation: "set_daily_limit", friendship_id: friendship, file_daily_limit_bytes: bytes }); },
  usage: async (friendship, signal) => {
    const value = await call({ operation: "usage", friendship_id: friendship }, signal);
    return record(value) ? parseQuota(value.quota) : null;
  },
  onFilesChanged: handler => listen<{ friendship_id?: unknown }>("neighbor-files-changed", event => {
    if (typeof event.payload?.friendship_id === "string") handler(event.payload.friendship_id);
  }),
};

export { formatBytes } from "../../lib/formatBytes";

/** Voice notes recorded in the composer are transcribed into the message rather than sent as files. */
export function isVoiceNote(path: string): boolean {
  return /(^|[\\/])voice-note-\d+\.[a-z0-9]+$/i.test(path);
}
