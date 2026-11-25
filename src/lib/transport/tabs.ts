import { parseCompositeTabId } from "../remoteTabTransport";
import { PeerTabRelayError } from "../remoteChatState";
import { childHistorySuffix, currentChildAuthority, type ChildTranscriptAuthority } from "../childTranscript";
import { backgroundProcessesSuffix, providerBackgroundOutputSuffix, type BackgroundProcessAuthority, type ProviderBackgroundWorkTarget } from "../backgroundProcesses";
import type { ShellCommandSnapshot } from "@codeterm/chat-core";
import type { ChatStartResult, ChatBackfillResult } from "../../components/Terminal/chatSessionAuthority";
import type { ChatResumeResult } from "../../components/Terminal/chatResume";
import type { ChatSendStatus } from "../../components/Terminal/chatSendStatus";
import type { SessionBindingFact } from "../../components/Terminal/sessionBindingSurface";
import type { ResizeOwnerClaim } from "../resizeOwner";
import { isPromptNotOnScreenResponse } from "../webApi";
import { hydrateMuxSnapshot } from "../muxRuntimeAssets";
import { RemotePtyIdCache, writeRemotePty, type RemotePtyWriteResponse } from "../remotePtyIdCache";
import type { AtomicSpawn, MultiplexerState, ShellProfile } from "../types";
import type { TransportClient } from "../transportClient";
import { REST_READ_TIMEOUT_MS } from "../restTimeoutPolicy";
export type TabsTransportClient = Pick<TransportClient, "json">;
type Args = Record<string, unknown> & {
    signal?: AbortSignal;
    callerTabId?: string;
};
const enc = encodeURIComponent;
const s = (args: Args, key: string, alias?: string): string => String(args[key] ?? (alias ? args[alias] : undefined) ?? "");
function exactPeerId(args: Args): string {
    const peerId = s(args, "peerId", "peer_id");
    if (!peerId.trim()) throw new Error("An exact peer installation id is required");
    return peerId;
}
function hasPeerTarget(args: Args): boolean {
    return args.peerId !== undefined || args.peer_id !== undefined;
}
export interface PtyCursors { offsets: Map<string, number>; truncations: Map<string, number> }
/** One relayed request on a paired peer; the peer's own refusal travels back as the error. */
export async function relayToPeer(client: TabsTransportClient, peer: string, method: string, path: string, body: unknown): Promise<unknown> {
    if (!peer.trim()) throw new Error("An exact peer installation id is required");
    const response = await client.json<{ ok?: boolean; data?: unknown; error?: string | null; peer?: { instance_id: string } }>("POST", "/api/mesh/relay-to", {
        target_instance_id: peer, method, path, body,
    });
    if (!response.ok) throw new Error(response.error ?? `${peer} did not answer ${method} ${path}`);
    if (response.peer?.instance_id !== peer) throw new Error("Relay answered for a different installation");
    return response.data;
}
export function createTabsTransport(client: TabsTransportClient, cursors: PtyCursors = { offsets: new Map(), truncations: new Map() }) {
    const webPtyOffsets = cursors.offsets;
    const remotePtyIds = new RemotePtyIdCache();
    const clientFor = (args: Args): TabsTransportClient => ({
        json: (method, path, body, opts) => client.json(method, path, body, {
            ...opts, signal: args.signal ?? opts?.signal, callerTabId: typeof args.callerTabId === "string" ? args.callerTabId : opts?.callerTabId,
        }),
    });
    return {
        provider_launch_profiles: (a: Args) => clientFor(a).json("GET", `/api/agents/providers/${enc(s(a, "providerId"))}/profiles`),
        provider_setup_profile: (a: Args) => clientFor(a).json("POST", `/api/agents/providers/${enc(s(a, "providerId"))}/profiles`, a.request, {timeoutMs: null}),
        provider_registry_search: (a: Args) => clientFor(a).json("GET", `/api/agents/providers/${enc(s(a, "providerId"))}/registry?q=${enc(s(a, "query"))}`),
        provider_registry_install: (a: Args) => clientFor(a).json("POST", `/api/agents/providers/${enc(s(a, "providerId"))}/registry/install`, {id: s(a, "id")}, {timeoutMs: null}),
        provider_remove_profile: (a: Args) => clientFor(a).json("DELETE", `/api/agents/providers/${enc(s(a, "providerId"))}/profiles/${enc(s(a, "name"))}`),
        wake_tab: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/wake`),
        mux_get_state: (a: Args = {}) => clientFor(a).json<MultiplexerState>("GET", "/api/mux/state").then(hydrateMuxSnapshot),
        agent_runtime_child_authority: async (a: Args) => {
            const state = hydrateMuxSnapshot(await clientFor(a).json<MultiplexerState>("GET", "/api/mux/state"));
            return currentChildAuthority(state.tabs[s(a, "tabId")]);
        },
        agent_runtime_answer_question: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent/runtime/questions/${enc(s(a, "itemId"))}/answer`, { answers: a.answers }),
        agent_runtime_goal: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent/runtime/goal`, a.request),
        agent_runtime_thread_history: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent/runtime/thread-history`, a.request),
        agent_runtime_reset_credits: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent/runtime/reset-credits`, a.request),
        agent_runtime_controls: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent/runtime/controls`, a.request),
        agent_runtime_host_approvals: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent/runtime/host-approvals`, a.request),
        agent_runtime_login: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent/runtime/login`, a.request),
        agent_runtime_retry: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent/runtime/retry`),
        agent_runtime_child_history: (a: Args) => clientFor(a).json("GET", `/api/tabs/${enc(s(a, "tabId"))}${childHistorySuffix(s(a,"threadId"), a.request as ChildTranscriptAuthority & {cursor?:string})}`),
        agent_runtime_background_processes: (a: Args) => clientFor(a).json("GET", `/api/tabs/${enc(s(a, "tabId"))}${backgroundProcessesSuffix(a.request as BackgroundProcessAuthority)}`),
        agent_runtime_stop_background_process: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent/runtime/background-processes`, a.request),
        agent_provider_background_output: (a: Args) => clientFor(a).json("GET", `/api/tabs/${enc(s(a, "tabId"))}${providerBackgroundOutputSuffix(a.request as ProviderBackgroundWorkTarget)}`),
        agent_provider_ask_stop_background_work: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent/background-work`, a.request),
        session_binding_state: (a: Args) => clientFor(a).json<SessionBindingFact>("GET", `/api/tabs/${enc(s(a, "tabId"))}/session/binding-state`),
        rebind_tab_session: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/session/rebind`, {
            session_id: s(a, "sessionId"),
        }),
        // Selection is this client's own; `/select` records it without moving any other client.
        mux_set_active_workspace: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/select`),
        mux_create_workspace: (a: Args) => clientFor(a).json("POST", "/api/workspaces", {
            name: a.name,
            cwd: a.cwd,
            activate: a.activate,
        }),
        mux_close_workspace: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/close`, {
            force: a.force === true,
            confirm: s(a, "workspaceId"),
        }),
        mux_set_workspace_cwd: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/cwd`, { cwd: a.cwd }),
        mux_rename_workspace: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/rename`, {
            name: a.name,
        }),
        shell_profiles: (a: Args) => clientFor(a).json<ShellProfile[]>("GET", `/api/shells?refresh=${a.refresh === true}`),
        mux_create_tab: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs`, {
            eager: a.eager === true,
            shell: a.shell,
            cwd: a.cwd,
            purpose: a.purpose,
            description: a.description,
            icon: a.icon,
            team_id: a.teamId,
            creator_tab_id: a.creatorTabId,
            cmd: a.cmd,
        }),
        mux_set_active_tab: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/select`),
        mux_set_active_tab_and_workspace: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/select`),
        mux_close_tab: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/close${a.force === true ? "?force=true" : ""}`),
        mux_reopen_closed_tab: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/reopen-closed`, {
            index: a.index,
        }),
        mux_reopen_closed_group: (a: Args) => clientFor(a).json("POST", `/api/closed-groups/${enc(s(a, "groupId"))}/reopen`),
        mux_rerun_restored_shell: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/rerun-restored-shell`),
        mux_restore_orphaned_tab: (a: Args) => clientFor(a).json("POST", `/api/mux/restore-orphaned`, { tab_id: s(a, "tabId") }),
        mux_rename_tab: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/rename`, { title: a.title }),
        mux_move_tab: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/move`, a),
        mux_reorder_tab: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/reorder`, {
            ordered_tab_ids: a.orderedTabIds,
        }),
        mux_reorder_child: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "parentTabId"))}/children/reorder`, {
            ordered_tab_ids: a.orderedTabIds,
        }),
        mux_reorder_display: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/display-order`, {
            ordered_tab_ids: a.orderedTabIds,
        }),
        mux_reorder_workspace: (a: Args) => clientFor(a).json("POST", `/api/workspaces/reorder`, {
            ordered_workspace_ids: a.orderedWorkspaceIds,
        }),
        mux_set_focused_tab: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/select`),
        mux_respond_prompt: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/prompt/respond`, a),
        mux_set_permission_auto_confirm: async (a: Args) => {
            const resp = await clientFor(a).json<{
                enabled?: boolean;
            }>("POST", `/api/tabs/${enc(s(a, "tabId"))}/permission-auto-confirm`, { enabled: a.enabled === true });
            return resp?.enabled === true;
        },
        mux_set_tab_agent_type: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent-type`, {
            agent_type: a.agentType,
        }),
        get_tab_text: async (a: Args) => {
            const resp = await clientFor(a).json<{
                text?: string;
            }>("GET", `/api/tabs/${enc(s(a, "tabId"))}/text?formatted=${a.formatted ? "true" : "false"}`);
            return resp.text ?? "";
        },
        shell_command_snapshot: (a: Args) => clientFor(a).json<ShellCommandSnapshot>("GET", `/api/tabs/${enc(s(a, "tabId"))}/shell-commands`),
        tab_todo_list: (a: Args) => clientFor(a).json("GET", `/api/tabs/${enc(s(a, "tabId"))}/todos`),
        tab_todo_review: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/todos/review`, a.request),
        chat_start: (a: Args) => clientFor(a).json<ChatStartResult>("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-raw/start`, {
            subscriptionId: s(a, "subscriptionId"),
            ...(a.knownWindow != null ? { knownWindow: a.knownWindow } : {}),
        }, { signal: a.signal as AbortSignal | undefined }),
        chat_stop: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-raw/stop`, {
            subscriptionId: s(a, "subscriptionId"),
        }),
        chat_backfill: (a: Args) => clientFor(a).json<ChatBackfillResult>("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-raw/backfill`, {
            before_offset: a.beforeOffset,
            offset_kind: a.offsetKind,
        }, { timeoutMs: REST_READ_TIMEOUT_MS }),
        chat_history_page: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-raw/history`, {
            source: s(a, "source"),
            session_id: s(a, "sessionId"),
            before_offset: a.beforeOffset,
        }, { timeoutMs: REST_READ_TIMEOUT_MS }),
        chat_resume: (a: Args) => clientFor(a).json<ChatResumeResult>("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-raw/resume`, {
            cursor: s(a, "cursor"),
        }),
        chat_send_text: (a: Args) => clientFor(a).json<ChatSendStatus>("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-send`, {
            text: a.text,
            client_token: a.clientToken,
            ...(a.expectedRecipient ? { expected_recipient: a.expectedRecipient } : {}),
        }),
        chat_send_dismissals: (a: Args) => clientFor(a).json("GET", `/api/tabs/${enc(s(a, "tabId"))}/chat-send/dismissals`),
        chat_send_delete: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-send/dismissals`, a.dismissal),
        chat_send_status: (a: Args) => clientFor(a).json<ChatSendStatus[]>("GET", `/api/tabs/${enc(s(a, "tabId"))}/chat-send`),
        chat_send_queue: (a: Args) => clientFor(a).json("GET", `/api/tabs/${enc(s(a, "tabId"))}/chat-send/queue`),
        chat_send_queue_change: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-send/queue`, a.action),
        chat_send_admissions: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-send/admissions`, { clientTokens: a.clientTokens }),
        ordinary_tabs_list: (a: Args = {}) => clientFor(a).json("GET", "/api/tabs"),
        mesh_peer_tabs_list: async (a: Args) => {
            const peer = s(a, "peerId", "peer_id");
            const response = await clientFor(a).json<{
                ok?: boolean;
                data?: MultiplexerState;
                error?: string | null;
                peer?: { instance_id: string; instance_name: string };
            }>("POST", "/api/mesh/relay-to", {
                target_instance_id: peer,
                method: "GET",
                path: "/api/mux/state",
                body: null,
            });
            if (!response.ok)
                throw new Error(response.error ?? "Failed to list remote Tabs");
            if (response.peer?.instance_id !== peer) throw new Error("Relay answered for a different installation");
            const snapshot = hydrateMuxSnapshot(response.data);
            if (!snapshot?.tabs || typeof snapshot.tabs !== "object") throw new Error("Remote peer returned no multiplexer snapshot");
            const tabs = Object.values(snapshot.tabs).filter(tab =>
                !tab.id.startsWith("mesh:") && !tab.remote_peer_id && !tab.remote_tab_id);
            const localTabIds = new Set(tabs.map(tab => tab.id));
            return {
                peer_id: peer,
                peer_name: response.peer.instance_name,
                tabs,
                tab_statuses: Object.fromEntries(Object.entries(snapshot.tab_statuses ?? {})
                    .filter(([tabId]) => localTabIds.has(tabId))),
            };
        },
        mesh_peer_workspaces_list: (a: Args) => relayToPeer(clientFor(a), s(a, "peerId", "peer_id"), "GET", "/api/workspaces", null),
        // The peer's published multiplexer state: the only read that names both its workspaces and Teams.
        mesh_peer_mux_state: (a: Args) => relayToPeer(clientFor(a), s(a, "peerId", "peer_id"), "GET", "/api/mux/state", null).then(hydrateMuxSnapshot),
        mesh_peer_notifications_list: (a: Args) => relayToPeer(clientFor(a), exactPeerId(a), "GET", "/api/notifications", null),
        mesh_peer_notification_action: (a: Args) => {
            const peerId = exactPeerId(a);
            const sourceId = s(a, "sourceId", "source_id");
            if (!sourceId.trim()) throw new Error("A source notification id is required");
            const id = enc(sourceId);
            const label = s(a, "label");
            switch (s(a, "notificationAction", "notification_action")) {
                case "read": return relayToPeer(clientFor(a), peerId, "POST", "/api/notifications/" + id + "/read", null);
                case "delete": return relayToPeer(clientFor(a), peerId, "DELETE", "/api/notifications/" + id, null);
                case "resolve": return relayToPeer(clientFor(a), peerId, "POST", "/api/notifications/" + id + "/resolve", null);
                case "answer":
                    if (!label.trim()) throw new Error("A notification answer label is required");
                    return relayToPeer(clientFor(a), peerId, "POST", "/api/notifications/" + id + "/answer", { label });
                default: throw new Error("Unsupported remote notification action");
            }
        },
        // A provisioned folder on the peer; its owner's active workspace stays where it is.
        mesh_peer_workspace_create: (a: Args) => relayToPeer(clientFor(a), s(a, "peerId", "peer_id"), "POST", "/api/workspaces", {
            name: s(a, "name"), provision: true, activate: false,
        }),
        mesh_remote_tab_action: async (a: Args) => {
            const peer = s(a, "peerId", "peer_id");
            const ordinary = s(a, "ordinaryTabId", "ordinary_tab_id");
            const identity = parseCompositeTabId(ordinary);
            if (!identity || identity.peerId !== peer) {
                throw new Error("Remote tab owner mismatch");
            }
            const suffix = s(a, "suffix");
            if (["/focus", "/select", "/agent/restart", "/close"].includes(suffix.split("?")[0])) {
                remotePtyIds.invalidate(ordinary);
            }
            const remoteTab = identity.remoteTabId;
            const resolvePtyId = async () => {
                const inspected = await clientFor(a).json<{
                    ok?: boolean;
                    data?: Array<{
                        pty_id?: string;
                    }>;
                }>("POST", "/api/mesh/relay-to", {
                    target_instance_id: peer,
                    method: "GET",
                    path: `/api/tabs/inspect?tab_ids=${enc(remoteTab)}`,
                    body: null,
                });
                return inspected.ok ? inspected.data?.[0]?.pty_id ?? "" : "";
            };
            if (suffix === "/pty/write") {
                return writeRemotePty<unknown>(remotePtyIds, ordinary, resolvePtyId, (ptyId) => clientFor(a).json<RemotePtyWriteResponse<unknown>>("POST", "/api/mesh/relay-to", {
                    target_instance_id: peer,
                    method: s(a, "method"),
                    path: `/api/pty/${enc(ptyId)}/write`,
                    body: a.body ?? null,
                }));
            }
            let path = suffix === "/runtime-authority" ? "/api/mux/state" : suffix === "/inspect"
                ? `/api/tabs/inspect?tab_ids=${enc(remoteTab)}`
                : `/api/tabs/${enc(remoteTab)}${suffix}`;
            if (suffix === "/pty/spawn") {
                path = "/api/pty/spawn";
                a.body = { ...(a.body as Record<string, unknown>), tab_id: remoteTab };
                remotePtyIds.invalidate(ordinary);
            }
            else if (suffix.startsWith("/pty/")) {
                const ptyId = await resolvePtyId();
                if (!ptyId)
                    throw new Error("Remote tab PTY unavailable");
                if (suffix === "/pty/resize")
                    path = `/api/pty/${enc(ptyId)}/resize`;
                else if (suffix === "/pty/claim-resize") {
                    path = `/api/tabs/${enc(remoteTab)}/resize-owner`;
                    a.body = { ...(a.body as Record<string, unknown>), pty_id: ptyId };
                }
                else if (suffix.startsWith("/pty/read"))
                    path = `/api/pty/${enc(ptyId)}/read${suffix.slice("/pty/read".length)}`;
                else
                    throw new Error("Unsupported remote PTY action");
            }
            const response = await clientFor(a).json("POST", "/api/mesh/relay-to", {
                target_instance_id: peer,
                method: s(a, "method"),
                path,
                body: a.body ?? null,
            }, suffix === "/chat-raw/backfill" || suffix === "/chat-raw/history"
                ? { timeoutMs: REST_READ_TIMEOUT_MS } : undefined);
            const relay = response as {
                ok?: boolean;
                status?: number;
                data?: unknown;
                error?: string | null;
            };
            if (suffix === "/prompt/respond" && relay.status === 409 && isPromptNotOnScreenResponse(relay.data)) {
                return relay.data;
            }
            if (!relay.ok) {
                const refusal = (relay.data as { error?: { code?: string; bytes_written?: boolean } } | null)?.error;
                throw new PeerTabRelayError(relay.status, "/api/mesh/relay-to", relay.error ?? "Remote tab unavailable",
                    refusal?.code, refusal?.bytes_written);
            }
            if (suffix === "/chat-send" && relay.data != null) {
                const project = (receipt: ChatSendStatus): ChatSendStatus => {
                    if (receipt?.tab_id !== remoteTab) throw new Error("Remote Chat send receipt tab mismatch");
                    return { ...receipt, tab_id: ordinary };
                };
                return Array.isArray(relay.data) ? relay.data.map(project) : project(relay.data as ChatSendStatus);
            }
            return relay.data;
        },
        spawn_pty: async (a: Args) => {
            const resp = await clientFor(a).json<{
                pty_id: string;
            }>("POST", "/api/pty/spawn", {
                command: a.command,
                cwd: a.cwd,
                cols: a.cols,
                rows: a.rows,
                predicted_id: a.predictedId,
                tab_id: a.tabId,
            });
            return resp.pty_id;
        },
        read_pty: async (a: Args) => {
            const ptyId = s(a, "ptyId");
            // First attach tails from 0 — the initial read doubles as history replay.
            const offset = webPtyOffsets.get(ptyId) ?? 0;
            const resp = await clientFor(a).json<{
                data: number[];
                offset: number;
                truncated_from?: number;
            }>("GET", `/api/pty/${enc(ptyId)}/read?offset=${offset}`);
            webPtyOffsets.set(ptyId, resp.offset);
            if (resp.truncated_from !== undefined) cursors.truncations.set(ptyId, resp.truncated_from);
            return resp.data;
        },
        write_pty_string: (a: Args) => clientFor(a).json("POST", `/api/pty/${enc(s(a, "ptyId"))}/write`, {
            data: a.data,
            raw: a.raw !== false,
        }),
        interrupt_agent: (a: Args) => a.tabId ? clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/stop`) :
            clientFor(a).json("POST", `/api/pty/${enc(s(a, "ptyId"))}/write`, {
                data: "\x1b",
                raw: true,
            }),
        resize_pty: (a: Args) => {
            if (!a.clientId)
                return Promise.resolve({ ok: true, ignored: true });
            return clientFor(a).json("POST", `/api/pty/${enc(s(a, "ptyId"))}/resize`, {
                cols: a.cols,
                rows: a.rows,
                client_id: a.clientId,
            });
        },
        claim_resize_owner: (a: Args) => clientFor(a).json<ResizeOwnerClaim>("POST", `/api/tabs/${enc(s(a, "tabId"))}/resize-owner`, {
            pty_id: a.ptyId,
            client_id: a.clientId,
            cols: a.cols,
            rows: a.rows,
            force: a.force === true,
        }),
        save_session: (a: Args = {}) => clientFor(a).json("POST", "/api/session/save"),
        restart_general_agent: (a: Args = {}) => clientFor(a).json("POST", "/api/agents/general/restart"),
        spawn_general_agent_session: (a: Args) => hasPeerTarget(a)
            ? relayToPeer(clientFor(a), exactPeerId(a), "POST", "/api/agents/general/sessions", a.params) as Promise<AtomicSpawn>
            : clientFor(a).json<AtomicSpawn>("POST", "/api/agents/general/sessions", a.params),
        provider_switch_status: (a: Args) => clientFor(a).json("GET", `/api/tabs/${enc(s(a, "tabId", "tab_id"))}/agent/switch-provider/${enc(s(a, "operationId"))}?wait=${a.wait === true}`),
        read_provider_history: (a: Args) => clientFor(a).json("GET", `/api/tabs/${enc(s(a, "tabId", "tab_id"))}/agent/provider-history?file=${enc(s(a, "file"))}`),
        switch_agent_provider: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId", "tab_id"))}/agent/switch-provider`, { provider: a.providerId, model: a.model, reasoning_effort: a.reasoningEffort, force: a.force === true }),
        restart_agent_tab: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId", "tab_id"))}/agent/restart`, { fresh: a.fresh === true, ...(a.providerId ? { provider: a.providerId } : {}) }),
        refresh_provider_capabilities: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId", "tab_id"))}/agent/capabilities/refresh`),
        fork_agent_tab: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId", "tab_id"))}/agent/fork`),
        switch_agent_model: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId", "tab_id"))}/agent/model`, {
            model: s(a, "model"),
            reasoning_effort: a.reasoningEffort,
            runtime_mode: a.runtimeMode,
            ...(a.serviceTier !== undefined ? { service_tier: a.serviceTier } : {}),
            ...(a.cancel === true ? { cancel: true } : {}),
            ...(a.restartResume === true ? { restart_resume: true } : {}),
        }),
        complete_general_agent_first_run: (a: Args) => clientFor(a).json("POST", "/api/agents/general/complete-first-run", {
            provider_id: s(a, "providerId", "provider_id"),
        }),
        mux_link_pty: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/link-pty`, {
            pty_id: a.ptyId,
        }),
        write_pty_launch_command: (a: Args) => clientFor(a).json("POST", `/api/pty/${enc(s(a, "ptyId"))}/launch-command`, {
            command: a.command,
        }),
        mux_update_tab_cwd: (a: Args & {
            tabId: string;
            cwd: string;
        }) => clientFor(a).json("POST", `/api/tabs/${enc(a.tabId)}/cwd`, { cwd: a.cwd }),
        get_tab_history_raw: async (a: Args & {
            tabId: string;
        }) => {
            const history = await clientFor(a).json<{
                output: string;
            }>("GET", `/api/tabs/${enc(a.tabId)}/history`);
            return history.output;
        },
        mux_spawn_tab: async (a: Args & {
            params: {
                agentId: string;
                workspaceId: string;
                cwd?: string;
                model?: string;
                reasoningEffort?: string;
                preset?: string;
                launchProfile?: string;
                accountId?: string;
                remoteWorkspaceId?: string;
            };
        }): Promise<AtomicSpawn | { tab_id: string; workspace_id?: string }> => {
            const p = a.params;
            if (hasPeerTarget(a)) {
                const result = await relayToPeer(clientFor(a), exactPeerId(a), "POST", "/api/agents/spawn", {
                    agent: p.agentId,
                    ...(p.remoteWorkspaceId ? { workspace_id: p.remoteWorkspaceId } : {}),
                    model: p.model,
                    reasoning_effort: p.reasoningEffort,
                    preset: p.preset,
                    launch_profile: p.launchProfile,
                });
                if (!result || typeof result !== "object" || typeof (result as { tab_id?: unknown }).tab_id !== "string") {
                    throw new Error("The selected machine did not return the created agent");
                }
                const created = result as { tab_id: string; workspace_id?: string };
                return { tab_id: created.tab_id, ...(created.workspace_id ? { workspace_id: created.workspace_id } : {}) };
            }
            const result = await clientFor(a).json<{
                tab_id: string;
                workspace_id?: string;
            }>("POST", "/api/agents/spawn", {
                agent: p.agentId, workspace_id: p.workspaceId, cwd: p.cwd, model: p.model,
                reasoning_effort: p.reasoningEffort, preset: p.preset, launch_profile: p.launchProfile,
                account_id: p.accountId,
            });
            const snapshot = hydrateMuxSnapshot(await clientFor(a).json<AtomicSpawn["snapshot"]>("GET", "/api/mux/state"));
            return { tab_id: result.tab_id, workspace_id: result.workspace_id ?? p.workspaceId, snapshot };
        },
    };
}
export type TabsTransport = ReturnType<typeof createTabsTransport>;
