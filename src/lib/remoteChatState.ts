export type RemoteChatPresence = "online" | "offline" | "not-paired" | "signed-out";
export type RemoteChatState = Exclude<RemoteChatPresence, "online"> | "unreachable" | "refused" | "too-old" | "generic";

export interface RemoteChatFailure {
  status?: number;
  code?: string;
  path?: string;
  raw: string;
  fromPeer?: boolean;
  neverWritten?: boolean;
  at: number;
}

export function remoteChatState(presence?: RemoteChatPresence, failure?: RemoteChatFailure): RemoteChatState | null {
  if (presence && presence !== "online") return presence;
  if (!failure) return null;
  const { status, code, raw, fromPeer } = failure;
  if (status === 404 && code === "peer_not_paired") return "not-paired";
  if (status === 403) return "refused";
  if (status === 426 || code === "version_unsupported" || code === "peer_relay_protocol_mismatch") return "too-old";
  if (status === 401 && fromPeer) return "signed-out";
  if ((status !== undefined && status >= 500) || /timeout|timed out|connection.refused|failed to fetch|networkerror|network error/i.test(raw)) return "unreachable";
  return "generic";
}

export const REMOTE_CHAT_COPY = {
  "not-paired": { label: "not paired", sentence: (name: string) => `${name} isn't paired with this computer anymore.`, sendReason: (name: string) => `${name} isn't paired`, action: "mesh" },
  offline: { label: "offline", sentence: (name: string) => `${name} is offline. Its chat appears here when it's back.`, sendReason: (name: string) => `${name} is offline`, action: null },
  unreachable: { label: "unreachable", sentence: (name: string) => `${name} isn't reachable right now.`, sendReason: (name: string) => `${name} isn't reachable`, action: "retry" },
  "signed-out": { label: "signed out", sentence: (name: string) => `${name} is signed out. Sign in on ${name} to continue.`, sendReason: (name: string) => `${name} is signed out`, action: null },
  refused: { label: "refused", sentence: (name: string) => `${name} didn't allow this chat.`, sendReason: (name: string) => `${name} didn't allow this chat`, action: "mesh" },
  "too-old": { label: "update required", sentence: (name: string) => `${name} needs an update to open this chat.`, sendReason: (name: string) => `${name} needs an update`, action: null },
  generic: { label: "unavailable", sentence: () => "Couldn't open this chat.", sendReason: () => "Couldn't open this chat", action: "retry" },
} as const;

export class PeerTabRelayError extends Error {
  constructor(public status: number | undefined, public path: string, public body: string,
    public code?: string, public bytesWritten?: boolean) {
    super(body);
  }
}

export function remoteChatFailure(error: unknown, at = Date.now()): RemoteChatFailure {
  const raw = error instanceof Error ? error.message : String(error);
  const transport = error as { status?: number; path?: string; body?: string } | null;
  const printed = raw.match(/(\/api\/[^\s]+) → HTTP (\d{3}):\s*/);
  const status = transport?.status ?? (printed ? Number(printed[2]) : undefined);
  const path = transport?.path ?? printed?.[1];
  const body = transport?.body ?? (printed ? raw.slice(printed[0].length) : raw);
  const relay = error instanceof PeerTabRelayError;
  const fields = (() => {
    try { return JSON.parse(body) as { code?: string; bytes_written?: boolean; error?: { code?: string; bytes_written?: boolean } }; }
    catch { return undefined; }
  })();
  const code = (relay ? error.code : undefined) ?? fields?.error?.code ?? fields?.code ?? body.match(/^(\w+):/)?.[1];
  const originRefusal = path === "/api/mesh/relay-to"
    && ((status === 404 && code === "peer_not_paired") || status === 403 || status === 426);
  return { status, path, code, raw, fromPeer: relay,
    neverWritten: relay
      ? error.bytesWritten === false || (error.bytesWritten == null && (code === "peer_relay_not_authorized" || code === "peer_relay_protocol_mismatch"))
      : originRefusal || (path === "/api/mesh/relay-to" && (fields?.error?.bytes_written ?? fields?.bytes_written) === false), at };
}

export function remoteMachineName(peerId: string, ...labels: Array<string | null | undefined>): string {
  return labels.map(label => label?.trim()).find(label => label && label !== peerId
    && !/^[\da-f]{8}(?:-[\da-f]{4}){3}-[\da-f]{12}$/i.test(label)) ?? "the other computer";
}
