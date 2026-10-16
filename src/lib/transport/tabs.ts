import { parseCompositeTabId } from "../remoteTabTransport";
import type { ShellCommandSnapshot } from "@codeterm/chat-core";
import type { ChatStartResult, ChatBackfillResult } from "../../components/Terminal/chatSessionAuthority";
import type { ChatResumeResult } from "../../components/Terminal/chatResume";
import type { ChatSendStatus } from "../../components/Terminal/chatSendStatus";
import type { SessionBindingFact } from "../../components/Terminal/sessionBindingSurface";
import type { ResizeOwnerClaim } from "../resizeOwner";
import { isPromptNotOnScreenResponse } from "../webApi";
import { RemotePtyIdCache, writeRemotePty, type RemotePtyWriteResponse } from "../remotePtyIdCache";
import type { AtomicSpawn, MultiplexerState, ShellProfile } from "../types";
import type { TransportClient } from "../transportClient";
export type TabsTransportClient = Pick<TransportClient, "json">;
type Args = Record<string, unknown> & {
    signal?: AbortSignal;
    callerTabId?: string;
};
const enc = encodeURIComponent;
const s = (args: Args, key: string, alias?: string): string => String(args[key] ?? (alias ? args[alias] : undefined) ?? "");
export interface PtyCursors { offsets: Map<string, number>; truncations: Map<string, number> }
export function createTabsTransport(client: TabsTransportClient, cursors: PtyCursors = { offsets: new Map(), truncations: new Map() }) {
    const webPtyOffsets = cursors.offsets;
    const remotePtyIds = new RemotePtyIdCache();
    const clientFor = (args: Args): TabsTransportClient => ({
        json: (method, path, body, opts) => client.json(method, path, body, {
            ...opts, signal: args.signal ?? opts?.signal, callerTabId: typeof args.callerTabId === "string" ? args.callerTabId : opts?.callerTabId,
        }),
    });
    return {
        wake_tab: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/wake`),
        mux_get_state: (a: Args = {}) => clientFor(a).json<MultiplexerState>("GET", "/api/mux/state"),
        agent_runtime_answer_question: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent/runtime/questions/${enc(s(a, "itemId"))}/answer`, { answers: a.answers }),
        agent_runtime_login: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent/runtime/login`, a.request),
        agent_runtime_retry: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/agent/runtime/retry`),
        session_binding_state: (a: Args) => clientFor(a).json<SessionBindingFact>("GET", `/api/tabs/${enc(s(a, "tabId"))}/session/binding-state`),
        rebind_tab_session: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/session/rebind`, {
            session_id: s(a, "sessionId"),
        }),
        mux_set_active_workspace: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/focus`),
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
        mux_set_active_tab: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/focus`),
        mux_set_active_tab_and_workspace: async (a: Args) => {
            await clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/focus`);
            return clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/focus`);
        },
        mux_close_tab: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/close`),
        mux_reopen_closed_tab: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/reopen-closed`, {
            index: a.index,
        }),
        mux_reopen_closed_group: (a: Args) => clientFor(a).json("POST", `/api/closed-groups/${enc(s(a, "groupId"))}/reopen`),
        mux_rerun_restored_shell: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/rerun-restored-shell`),
        mux_restore_orphaned_tab: (a: Args) => clientFor(a).json("POST", `/api/mux/restore-orphaned`, { tab_id: s(a, "tabId") }),
        mux_rename_tab: (a: Args) => clientFor(a).json("POST", `/api/workspaces/${enc(s(a, "workspaceId"))}/tabs/${enc(s(a, "tabId"))}/rename`, { name: a.name }),
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
        mux_set_focused_tab: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/focus`),
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
        chat_start: (a: Args) => clientFor(a).json<ChatStartResult>("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-raw/start`, {
            subscriptionId: s(a, "subscriptionId"),
        }),
        chat_stop: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-raw/stop`, {
            subscriptionId: s(a, "subscriptionId"),
        }),
        chat_backfill: (a: Args) => clientFor(a).json<ChatBackfillResult>("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-raw/backfill`, {
            before_offset: a.beforeOffset,
            offset_kind: a.offsetKind,
        }),
        chat_resume: (a: Args) => clientFor(a).json<ChatResumeResult>("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-raw/resume`, {
            cursor: s(a, "cursor"),
        }),
        chat_send_text: (a: Args) => clientFor(a).json<ChatSendStatus>("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-send`, {
            text: a.text,
            client_token: a.clientToken,
        }),
        chat_send_dismissals: (a: Args) => clientFor(a).json("GET", `/api/tabs/${enc(s(a, "tabId"))}/chat-send/dismissals`),
        chat_send_delete: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-send/dismissals`, a.dismissal),
        chat_send_status: (a: Args) => clientFor(a).json<ChatSendStatus[]>("GET", `/api/tabs/${enc(s(a, "tabId"))}/chat-send`),
        chat_send_admissions: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId"))}/chat-send/admissions`, { clientTokens: a.clientTokens }),
        ordinary_tabs_list: (a: Args = {}) => clientFor(a).json("GET", "/api/tabs"),
        mesh_peer_tabs_list: async (a: Args) => {
            const peer = s(a, "peerId", "peer_id");
            const response = await clientFor(a).json<{
                ok?: boolean;
                data?: unknown;
                error?: string | null;
            }>("POST", "/api/mesh/relay-to", {
                target_peer_id: peer,
                method: "GET",
                path: "/api/tabs?local_only=true",
                body: null,
            });
            if (!response.ok)
                throw new Error(response.error ?? "Failed to list remote Tabs");
            const peers = await clientFor(a).json<{
                paired?: Array<{
                    instance_id: string;
                    instance_name: string;
                }>;
            }>("GET", "/api/mesh/peers");
            const peerName = peers.paired?.find((item) => item.instance_id === peer)?.instance_name ?? peer;
            return { peer_id: peer, peer_name: peerName, tabs: response.data ?? [] };
        },
        mesh_remote_tab_action: async (a: Args) => {
            const peer = s(a, "peerId", "peer_id");
            const ordinary = s(a, "ordinaryTabId", "ordinary_tab_id");
            const identity = parseCompositeTabId(ordinary);
            if (!identity || identity.peerId !== peer) {
                throw new Error("Remote tab owner mismatch");
            }
            if (["/focus", "/agent/restart", "/close"].includes(s(a, "suffix"))) {
                remotePtyIds.invalidate(ordinary);
            }
            const remoteTab = identity.remoteTabId;
            const suffix = s(a, "suffix");
            const resolvePtyId = async () => {
                const inspected = await clientFor(a).json<{
                    ok?: boolean;
                    data?: Array<{
                        pty_id?: string;
                    }>;
                }>("POST", "/api/mesh/relay-to", {
                    target_peer_id: peer,
                    method: "GET",
                    path: `/api/tabs/inspect?tab_ids=${enc(remoteTab)}`,
                    body: null,
                });
                return inspected.ok ? inspected.data?.[0]?.pty_id ?? "" : "";
            };
            if (suffix === "/pty/write") {
                return writeRemotePty<unknown>(remotePtyIds, ordinary, resolvePtyId, (ptyId) => clientFor(a).json<RemotePtyWriteResponse<unknown>>("POST", "/api/mesh/relay-to", {
                    target_peer_id: peer,
                    method: s(a, "method"),
                    path: `/api/pty/${enc(ptyId)}/write`,
                    body: a.body ?? null,
                }));
            }
            let path = suffix === "/inspect"
                ? `/api/tabs/inspect?tab_ids=${enc(remoteTab)}`
                : `/api/tabs/${enc(remoteTab)}${suffix}`;
            if (suffix.startsWith("/pty/")) {
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
                target_peer_id: peer,
                method: s(a, "method"),
                path,
                body: a.body ?? null,
            });
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
                throw new Error(relay.error ?? "Remote tab unavailable");
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
        spawn_general_agent_session: (a: Args) => clientFor(a).json<AtomicSpawn>("POST", "/api/agents/general/sessions", a.params),
        restart_agent_tab: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId", "tab_id"))}/agent/restart`, { fresh: a.fresh === true, ...(a.providerId ? { provider: a.providerId } : {}) }),
        refresh_provider_capabilities: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId", "tab_id"))}/agent/capabilities/refresh`),
        fork_agent_tab: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId", "tab_id"))}/agent/fork`),
        switch_agent_model: (a: Args) => clientFor(a).json("POST", `/api/tabs/${enc(s(a, "tabId", "tab_id"))}/agent/model`, {
            model: s(a, "model"),
            reasoning_effort: a.reasoningEffort,
            runtime_mode: a.runtimeMode,
            ...(a.cancel === true ? { cancel: true } : {}),
            ...(a.restartResume === true ? { restart_resume: true } : {}),
        }),
        complete_general_agent_first_run: (a: Args) => clientFor(a).json("POST", "/api/agents/general/complete-first-run", {
            provider_id: s(a, "providerId", "provider_id"),
            full_access: a.fullAccess === true || a.full_access === true,
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
            };
        }): Promise<AtomicSpawn> => {
            const p = a.params;
            const result = await clientFor(a).json<{
                tab_id: string;
                workspace_id?: string;
            }>("POST", "/api/agents/spawn", {
                agent: p.agentId, workspace_id: p.workspaceId, cwd: p.cwd, model: p.model,
                reasoning_effort: p.reasoningEffort, preset: p.preset, activate: true,
            });
            const snapshot = await clientFor(a).json<AtomicSpawn["snapshot"]>("GET", "/api/mux/state");
            return { tab_id: result.tab_id, workspace_id: result.workspace_id ?? p.workspaceId, snapshot };
        },
    };
}
export type TabsTransport = ReturnType<typeof createTabsTransport>;
