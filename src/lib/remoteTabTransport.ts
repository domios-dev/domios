import type { ApiConfig } from "./types";
import { daemonAuthToken, daemonHttpBase } from "./webApi";

export interface RemoteTransportCall {
  ordinaryTabId: string;
  peerId: string;
  remoteTabId: string;
  method: string;
  suffix: string;
  body: unknown;
  select?: (value: unknown) => unknown;
}

export interface RemoteTabIdentity {
  ordinaryTabId: string;
  peerId: string;
  remoteTabId: string;
}

export type TabTarget =
  | { kind: "local" }
  | { kind: "remote"; ordinaryTabId: string; peerId: string }
  | { kind: "invalid-remote"; value: string };

const readOffsets = new Map<string, number>();
let apiConfig: ApiConfig | undefined;
const credentialListeners = new Set<() => void>();
let credentials: string | null = null;

/** Streams authenticate with the daemon token from config, which is null in the
 *  pre-load default; the 401 that darkens a channel only revives on a signal. */
export function onRemoteTransportCredentials(listener: () => void): () => void {
  credentialListeners.add(listener);
  return () => {
    credentialListeners.delete(listener);
  };
}

function credentialsOf(api: ApiConfig | undefined): string {
  return `${api?.host ?? ""}:${api?.port ?? ""}:${api?.token ?? ""}`;
}

export function configureRemoteTabTransport(api: ApiConfig | undefined): void {
  apiConfig = api;
  const next = credentialsOf(api);
  if (next === credentials) return;
  credentials = next;
  for (const listener of credentialListeners) listener();
}

export function remoteTabTransportHttp(): { base: string; token: string } {
  return { base: daemonHttpBase(apiConfig), token: daemonAuthToken(apiConfig) };
}

export function parseCompositeTabId(value: unknown): RemoteTabIdentity | null {
  if (typeof value !== "string" || !value.startsWith("mesh:")) return null;
  const slash = value.indexOf("/", 5);
  if (slash < 6 || slash === value.length - 1 || value.indexOf("/", slash + 1) >= 0) return null;
  if (/[?&#\s]/.test(value)) return null;
  return {
    ordinaryTabId: value,
    peerId: value.slice(5, slash),
    remoteTabId: value.slice(slash + 1),
  };
}

export function classifyTabTarget(value: unknown): TabTarget {
  if (typeof value !== "string" || !value.startsWith("mesh:")) return { kind: "local" };
  const parsed = parseCompositeTabId(value);
  return parsed ? { kind: "remote", ...parsed } : { kind: "invalid-remote", value };
}

export function remoteTransportCall(command: string, args: Record<string, unknown> = {}): RemoteTransportCall | null {
  const identity = parseCompositeTabId(args.tabId ?? args.tab_id ?? args.ptyId ?? args.pty_id);
  if (!identity) return null;
  const call = (method: string, suffix: string, body: unknown = null, select?: (value: unknown) => unknown): RemoteTransportCall => ({
    ...identity, method, suffix, body, select,
  });
  switch (command) {
    case "mux_close_tab": return call("POST", "/close");
    case "mux_rename_tab": return call("POST", "/rename", { title: args.title ?? args.name });
    case "mux_set_focused_tab": return call("POST", "/focus");
    case "get_tab_history_raw":
    case "get_tab_output": return call("GET", "/output", null, (value) => (value as { text?: string; output?: string })?.text ?? (value as { output?: string })?.output ?? "");
    case "get_tab_text": return call("GET", `/text?formatted=${args.formatted === true}`, null, (value) => (value as { text?: string })?.text ?? "");
    case "write_pty_string": return call("POST", "/pty/write", { data: args.data, raw: args.raw !== false });
    case "interrupt_agent": return call("POST", "/pty/write", { data: "\u001b", raw: true });
    case "resize_pty": return call("POST", "/pty/resize", { cols: args.cols, rows: args.rows, client_id: args.clientId });
    case "claim_resize_owner": return call("POST", "/pty/claim-resize", {
      pty_id: args.ptyId, client_id: args.clientId, cols: args.cols, rows: args.rows, force: args.force === true,
    });
    case "read_pty": {
      const offset = readOffsets.get(identity.ordinaryTabId) ?? 0;
      return call("GET", `/pty/read?offset=${offset}`, null, (value) => {
        const response = value as { data?: number[]; offset?: number };
        if (typeof response.offset === "number") readOffsets.set(identity.ordinaryTabId, response.offset);
        return response.data ?? [];
      });
    }
    case "chat_start": return call("POST", "/chat-raw/start", { subscriptionId: args.subscriptionId });
    case "chat_stop": return call("POST", "/chat-raw/stop", { subscriptionId: args.subscriptionId });
    case "chat_backfill": return call("POST", "/chat-raw/backfill", { before_offset: args.beforeOffset, max_bytes: args.maxBytes });
    case "chat_send_text": return call("POST", "/chat-send", {
      text: args.text,
      client_token: args.clientToken,
    });
    case "chat_send_status": return call("GET", "/chat-send");
    case "shell_command_snapshot": return call("GET", "/shell-commands");
    case "delayed_send_list": return call("GET", "/delayed-messages");
    case "delayed_send_create": return call("POST", "/delayed-messages", {
      text: args.text,
      scheduled_at_ms: args.scheduledAtMs,
    });
    case "mux_respond_prompt": return call("POST", "/prompt/respond", args);
    case "restart_agent_tab": return call("POST", "/agent/restart");
    default: return null;
  }
}

export function hasCompositeTabArgument(args: Record<string, unknown> = {}): boolean {
  return classifyTabTarget(args.tabId ?? args.tab_id ?? args.ptyId ?? args.pty_id).kind !== "local";
}
