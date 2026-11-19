import type { HttpMethod, TransportClient } from "../transportClient";
import { describe, expect, it } from "vitest";
import { remoteTransportCall } from "../remoteTabTransport";
import { createTabsTransport, type TabsTransportClient } from "./tabs";

it("sends checklist review snapshot and amendment through the authenticated tab transport", async () => {
  const { transport, calls } = fixture();
  const request = { generation: 2, revision: 8, items: [], note: "Simplify" };
  await transport.tab_todo_review({ tabId: "worker", callerTabId: "parent", request });
  expect(calls[0]).toMatchObject({ method: "POST", path: "/api/tabs/worker/todos/review", body: request, opts: { callerTabId: "parent" } });
});

it("sends a tab rename as the title field the daemon requires", async () => {
  const { transport, calls } = fixture();
  await transport.mux_rename_tab({ workspaceId: "ws", tabId: "tab", title: "Renamed" });
  expect(calls[0]).toMatchObject({ method: "POST", path: "/api/workspaces/ws/tabs/tab/rename", body: { title: "Renamed" } });
});

it("carries the client's known window to chat attach", async () => {
  const { transport, calls } = fixture();
  const knownWindow = { source_session_id: "own", session_binding_nonce: "nonce", end_offset: 123, file_length: 125, cursor: "opaque" };
  await transport.chat_start({ tabId: "tab", subscriptionId: "sub", knownWindow });
  expect(calls[0].body).toEqual({ subscriptionId: "sub", knownWindow });
});

it("relays General creation to the exact peer with a peer-default request", async () => {
  const { transport, calls } = fixture([{ ok: true, data: { tab_id: "general-tab" }, peer: { instance_id: "machine-x" } }]);
  const params = { role: "general_agent", intent: "fresh_general" };
  await transport.spawn_general_agent_session({ peerId: "machine-x", params });
  expect(calls).toHaveLength(1);
  expect(calls[0]).toMatchObject({
    method: "POST",
    path: "/api/mesh/relay-to",
    body: { target_instance_id: "machine-x", method: "POST", path: "/api/agents/general/sessions", body: params },
  });
});

it("relays a single agent to the exact peer without leaking the origin workspace or cwd", async () => {
  const { transport, calls } = fixture([{ ok: true, data: { tab_id: "remote-agent" }, peer: { instance_id: "machine-x" } }]);
  const result = await transport.mux_spawn_tab({ peerId: "machine-x", params: {
    agentId: "codex", workspaceId: "origin-workspace", cwd: "C:/origin/repo", model: "model-x", reasoningEffort: "high",
  } });
  expect(result).toEqual({ tab_id: "remote-agent" });
  expect(calls).toHaveLength(1);
  expect(calls[0]).toMatchObject({
    method: "POST",
    path: "/api/mesh/relay-to",
    body: {
      target_instance_id: "machine-x", method: "POST", path: "/api/agents/spawn",
      body: { agent: "codex", model: "model-x", reasoning_effort: "high" },
    },
  });
  const relayBody = calls[0].body as { body: Record<string, unknown> };
  expect(relayBody.body).not.toHaveProperty("workspace_id");
  expect(relayBody.body).not.toHaveProperty("cwd");
  expect(relayBody.body).not.toHaveProperty("role");
});

it("uses a selected workspace from the target machine when provided", async () => {
  const { transport, calls } = fixture([{ ok: true, data: { tab_id: "remote-agent" }, peer: { instance_id: "machine-x" } }]);
  await transport.mux_spawn_tab({ peerId: "machine-x", params: {
    agentId: "codex", workspaceId: "origin-workspace", remoteWorkspaceId: "peer-workspace",
  } });
  const relay = calls[0].body as { target_instance_id: string; body: Record<string, unknown> };
  expect(relay.target_instance_id).toBe("machine-x");
  expect(relay.body).toMatchObject({ agent: "codex", workspace_id: "peer-workspace" });
  expect(relay.body).not.toHaveProperty("cwd");
});

it("surfaces a peer refusal without making a local spawn request", async () => {
  const { transport, calls } = fixture([{ ok: false, error: "OFFLINE", peer: { instance_id: "machine-x" } }]);
  await expect(transport.mux_spawn_tab({ peerId: "machine-x", params: { agentId: "codex", workspaceId: "origin-workspace" } }))
    .rejects.toThrow("OFFLINE");
  expect(calls).toHaveLength(1);
  expect(calls[0].body).toMatchObject({ target_instance_id: "machine-x", path: "/api/agents/spawn" });
});

it("surfaces a peer General refusal without making a local session request", async () => {
  const { transport, calls } = fixture([{ ok: false, error: "PEER-REFUSAL", peer: { instance_id: "machine-x" } }]);
  await expect(transport.spawn_general_agent_session({ peerId: "machine-x", params: { role: "general_agent", intent: "fresh_general" } }))
    .rejects.toThrow("PEER-REFUSAL");
  expect(calls).toHaveLength(1);
  expect(calls[0].body).toMatchObject({ target_instance_id: "machine-x", path: "/api/agents/general/sessions" });
});

it("does not reinterpret an empty remote target as a local General request", async () => {
  const { transport, calls } = fixture();
  expect(() => transport.spawn_general_agent_session({ peerId: "", params: { role: "general_agent", intent: "fresh_general" } }))
    .toThrow("exact peer installation id");
  expect(calls).toHaveLength(0);
});

function fixture(responses: unknown[] = [{}]) {
  const calls: Array<{ method: string; path: string; body: unknown; opts: unknown }> = [];
  const client: TabsTransportClient = {
    async json<T>(method: HttpMethod, path: string, body?: unknown, opts?: Parameters<TransportClient["json"]>[3]): Promise<T> {
      calls.push({ method, path, body, opts });
      if (opts?.signal?.aborted) throw opts.signal.reason;
      const response = responses.length > 1 ? responses.shift() : responses[0];
      if (response instanceof Error) throw response;
      return response as T;
    },
  };
  return { calls, transport: createTabsTransport(client) };
}

it("reads current child authority from the daemon snapshot", async()=>{
  const {transport,calls}=fixture([{tabs:{owned:{agent_session_id:"root",session_binding_nonce:"restored",agent_runtime_state:{state:"loaded",epoch:9}}}}]);
  expect(await transport.agent_runtime_child_authority({tabId:"owned"})).toEqual({rootThreadId:"root",bindingNonce:"restored",epoch:9});
  expect(calls[0]).toMatchObject({method:"GET",path:"/api/mux/state"});
});

it("relays a child authority snapshot read to the owning peer", async()=>{
  const {transport,calls}=fixture([{ok:true,data:{tabs:{}}}]);
  await transport.mesh_remote_tab_action({ordinaryTabId:"mesh:peer/owned",peerId:"peer",method:"GET",suffix:"/runtime-authority"});
  expect(calls[0]).toMatchObject({method:"POST",path:"/api/mesh/relay-to",body:{target_instance_id:"peer",method:"GET",path:"/api/mux/state",body:null}});
});

it("lists notification history from the exact paired installation", async () => {
  const rows = [{ id: "same-source-id" }];
  const { transport, calls } = fixture([{ ok: true, data: rows, peer: { instance_id: "install-a", instance_name: "Same name" } }]);
  expect(await transport.mesh_peer_notifications_list({ peerId: "install-a" })).toEqual(rows);
  expect(calls[0]).toMatchObject({
    method: "POST",
    path: "/api/mesh/relay-to",
    body: { target_instance_id: "install-a", method: "GET", path: "/api/notifications", body: null },
  });
});

it("lists attached peer tabs with only their authoritative status snapshot", async () => {
  const { transport, calls } = fixture([{
    ok: true,
    peer: { instance_id: "install-b", instance_name: "Peer B" },
    data: {
      tabs: {
        "remote-chat": { id: "remote-chat", pty_id: null, tab_type: "Terminal", title: "Chat", cwd: null },
        "mesh:install-a/attached": { id: "mesh:install-a/attached", pty_id: "mesh:install-a/attached", tab_type: "Terminal", title: "Other", cwd: null },
        "nested-remote": { id: "nested-remote", pty_id: null, tab_type: "Terminal", title: "Nested", cwd: null, remote_peer_id: "install-a", remote_tab_id: "source" },
      },
      tab_statuses: { "remote-chat": "Working", "mesh:install-a/attached": "Idle", "nested-remote": "Idle", unrelated: "Error" },
    },
  }]);

  expect(await transport.mesh_peer_tabs_list({ peerId: "install-b" })).toEqual({
    peer_id: "install-b",
    peer_name: "Peer B",
    tabs: [{ id: "remote-chat", pty_id: null, tab_type: "Terminal", title: "Chat", cwd: null }],
    tab_statuses: { "remote-chat": "Working" },
  });
  expect(calls[0]).toMatchObject({
    method: "POST",
    path: "/api/mesh/relay-to",
    body: { target_instance_id: "install-b", method: "GET", path: "/api/mux/state", body: null },
  });
});

it.each([
  ["read", "POST", "/api/notifications/source%2Fid/read", null],
  ["delete", "DELETE", "/api/notifications/source%2Fid", null],
  ["resolve", "POST", "/api/notifications/source%2Fid/resolve", null],
  ["answer", "POST", "/api/notifications/source%2Fid/answer", { label: "Approve" }],
] as const)("routes remote notification %s by exact peer and source ids", async (notificationAction, method, path, body) => {
  const { transport, calls } = fixture([{ ok: true, data: { ok: true }, peer: { instance_id: "install-b", instance_name: "Same name" } }]);
  await transport.mesh_peer_notification_action({
    peerId: "install-b",
    sourceId: "source/id",
    notificationAction,
    ...(notificationAction === "answer" ? { label: "Approve" } : {}),
  });
  expect(calls[0]).toMatchObject({
    method: "POST",
    path: "/api/mesh/relay-to",
    body: { target_instance_id: "install-b", method, path, body },
  });
});

it("refuses an empty or mismatched remote notification installation before accepting another origin", async () => {
  const { transport, calls } = fixture([{ ok: true, data: { ok: true }, peer: { instance_id: "install-a", instance_name: "Same name" } }]);
  expect(() => transport.mesh_peer_notification_action({
    peerId: "",
    sourceId: "same-source-id",
    notificationAction: "read",
  })).toThrow("exact peer installation");
  await expect(transport.mesh_peer_notification_action({
    peerId: "install-b",
    sourceId: "same-source-id",
    notificationAction: "read",
  })).rejects.toThrow("different installation");
  expect(calls).toHaveLength(1);
  expect(calls[0].body).toMatchObject({ target_instance_id: "install-b" });
});

it("starts or reuses a remote tab's PTY on its own peer without a prior PTY lookup", async()=>{
  const {transport,calls}=fixture([{ok:true,data:{pty_id:"peer-pty"}}]);
  await transport.mesh_remote_tab_action({ordinaryTabId:"mesh:peer/shell",peerId:"peer",method:"POST",suffix:"/pty/spawn",body:{cols:80,rows:24}});
  expect(calls).toHaveLength(1);
  expect(calls[0]).toMatchObject({method:"POST",path:"/api/mesh/relay-to",body:{target_instance_id:"peer",method:"POST",path:"/api/pty/spawn",body:{cols:80,rows:24,tab_id:"shell"}}});
});

it("reads child history with root authority and no send payload",async()=>{
  const {transport,calls}=fixture();
  await transport.agent_runtime_child_history({tabId:"owned",threadId:"child/a",request:{rootThreadId:"root",bindingNonce:"nonce",epoch:4}});
  expect(calls).toHaveLength(1);
  expect(calls[0]).toMatchObject({method:"GET",path:"/api/tabs/owned/agent/runtime/children/child%2Fa/history?rootThreadId=root&bindingNonce=nonce&epoch=4",body:undefined});
});

it("lists owned background processes and stops one through the same bound thread", async () => {
  const request = {rootThreadId:"root",bindingNonce:"nonce",epoch:4};
  const {transport,calls}=fixture();
  await transport.agent_runtime_background_processes({tabId:"owned",request});
  await transport.agent_runtime_stop_background_process({tabId:"owned",request:{...request,threadId:"child",processId:"process"}});
  expect(calls).toMatchObject([
    {method:"GET",path:"/api/tabs/owned/agent/runtime/background-processes?rootThreadId=root&bindingNonce=nonce&epoch=4",body:undefined},
    {method:"POST",path:"/api/tabs/owned/agent/runtime/background-processes",body:{...request,threadId:"child",processId:"process"}},
  ]);
});

const simple = [
  ["wake_tab", "POST", "/api/tabs/tab/wake"],
  ["agent_runtime_answer_question", "POST", "/api/tabs/tab/agent/runtime/questions/question/answer"],
  ["agent_runtime_goal", "POST", "/api/tabs/tab/agent/runtime/goal"],
  ["agent_runtime_thread_history", "POST", "/api/tabs/tab/agent/runtime/thread-history"],
  ["agent_runtime_reset_credits", "POST", "/api/tabs/tab/agent/runtime/reset-credits"],
  ["agent_runtime_login", "POST", "/api/tabs/tab/agent/runtime/login"],
  ["agent_runtime_retry", "POST", "/api/tabs/tab/agent/runtime/retry"],
  ["chat_send_admissions", "POST", "/api/tabs/tab/chat-send/admissions"],
  ["mux_get_state", "GET", "/api/mux/state"],
  ["session_binding_state", "GET", "/api/tabs/tab/session/binding-state"],
  ["rebind_tab_session", "POST", "/api/tabs/tab/session/rebind"],
  ["mux_set_active_workspace", "POST", "/api/workspaces/ws/select"],
  ["mux_create_workspace", "POST", "/api/workspaces"],
  ["mux_close_workspace", "POST", "/api/workspaces/ws/close"],
  ["mux_set_workspace_cwd", "POST", "/api/workspaces/ws/cwd"],
  ["mux_rename_workspace", "POST", "/api/workspaces/ws/rename"],
  ["shell_profiles", "GET", "/api/shells?refresh=false"],
  ["mux_create_tab", "POST", "/api/workspaces/ws/tabs"],
  ["mux_set_active_tab", "POST", "/api/workspaces/ws/tabs/tab/select"],
  ["mux_set_active_tab_and_workspace", "POST", "/api/workspaces/ws/tabs/tab/select"],
  ["mux_close_tab", "POST", "/api/workspaces/ws/tabs/tab/close"],
  ["mux_reopen_closed_tab", "POST", "/api/workspaces/ws/reopen-closed"],
  ["mux_reopen_closed_group", "POST", "/api/closed-groups/group/reopen"],
  ["mux_rerun_restored_shell", "POST", "/api/tabs/tab/rerun-restored-shell"],
  ["mux_restore_orphaned_tab", "POST", "/api/mux/restore-orphaned"],
  ["mux_rename_tab", "POST", "/api/workspaces/ws/tabs/tab/rename"],
  ["mux_move_tab", "POST", "/api/workspaces/ws/tabs/tab/move"],
  ["mux_reorder_tab", "POST", "/api/workspaces/ws/tabs/reorder"],
  ["mux_reorder_child", "POST", "/api/tabs/parent/children/reorder"],
  ["mux_reorder_display", "POST", "/api/workspaces/ws/tabs/display-order"],
  ["mux_reorder_workspace", "POST", "/api/workspaces/reorder"],
  ["mux_set_focused_tab", "POST", "/api/tabs/tab/select"],
  ["mux_respond_prompt", "POST", "/api/tabs/tab/prompt/respond"],
  ["mux_set_tab_agent_type", "POST", "/api/tabs/tab/agent-type"],
  ["shell_command_snapshot", "GET", "/api/tabs/tab/shell-commands"],
  ["chat_start", "POST", "/api/tabs/tab/chat-raw/start"],
  ["chat_stop", "POST", "/api/tabs/tab/chat-raw/stop"],
  ["chat_backfill", "POST", "/api/tabs/tab/chat-raw/backfill"],
  ["chat_send_text", "POST", "/api/tabs/tab/chat-send"],
  ["chat_send_dismissals", "GET", "/api/tabs/tab/chat-send/dismissals"],
  ["chat_send_delete", "POST", "/api/tabs/tab/chat-send/dismissals"],
  ["chat_send_status", "GET", "/api/tabs/tab/chat-send"],
  ["ordinary_tabs_list", "GET", "/api/tabs"],
  ["write_pty_string", "POST", "/api/pty/pty/write"],
  ["interrupt_agent", "POST", "/api/tabs/tab/stop"],
  ["resize_pty", "POST", "/api/pty/pty/resize"],
  ["claim_resize_owner", "POST", "/api/tabs/tab/resize-owner"],
  ["save_session", "POST", "/api/session/save"],
  ["restart_general_agent", "POST", "/api/agents/general/restart"],
  ["spawn_general_agent_session", "POST", "/api/agents/general/sessions"],
  ["restart_agent_tab", "POST", "/api/tabs/tab/agent/restart"],
  ["read_provider_history", "GET", "/api/tabs/tab/agent/provider-history?file="],
  ["switch_agent_provider", "POST", "/api/tabs/tab/agent/switch-provider"],
  ["refresh_provider_capabilities", "POST", "/api/tabs/tab/agent/capabilities/refresh"],
  ["fork_agent_tab", "POST", "/api/tabs/tab/agent/fork"],
  ["switch_agent_model", "POST", "/api/tabs/tab/agent/model"],
  ["complete_general_agent_first_run", "POST", "/api/agents/general/complete-first-run"],
  ["mux_link_pty", "POST", "/api/tabs/tab/link-pty"],
  ["write_pty_launch_command", "POST", "/api/pty/pty/launch-command"],
] as const;

const args = { tabId: "tab", itemId: "question", workspaceId: "ws", parentTabId: "parent", groupId: "group", ptyId: "pty", clientId: "client", callerTabId: "caller" };

describe("tab command HTTP contract", () => {
  it.each(simple)("%s resolves to %s %s with caller routing", async (name, method, path) => {
    const { transport, calls } = fixture();
    await transport[name](args);
    expect(calls).toHaveLength(1);
    expect(calls[0]).toMatchObject({ method, path, opts: { callerTabId: "caller" } });
  });

  it("preserves reset confirmation and retry UUID", async () => {
    const {transport,calls}=fixture();
    const request={action:"redeem",confirmed:true,idempotencyKey:"confirmation-key"};
    await transport.agent_runtime_reset_credits({tabId:"owned",request});
    expect(calls[0]).toMatchObject({method:"POST",path:"/api/tabs/owned/agent/runtime/reset-credits",body:request});
  });

  it("preserves captured prompt identity and native value on the existing response route", async () => {
    const { transport, calls } = fixture();
    const identity = { kind: "structured", options: [{ label: "display", value: "native" }], request_id: "opaque" };
    await transport.mux_respond_prompt({ tabId: "owned", value: "native", identity });
    expect(calls[0]).toMatchObject({ method: "POST", path: "/api/tabs/owned/prompt/respond", body: { tabId: "owned", value: "native", identity } });
  });

  it("keeps runtime question answers on their item-scoped ledger route", async () => {
    const receipt = { send_id: "receipt", state: "written" };
    const { transport, calls } = fixture([receipt]);
    const answers = [{ questionId: "q", values: ["native-value"], text: "free text" }];
    expect(await transport.agent_runtime_answer_question({ tabId: "owned", itemId: "item/a", answers })).toBe(receipt);
    expect(calls[0]).toMatchObject({ method: "POST", path: "/api/tabs/owned/agent/runtime/questions/item%2Fa/answer", body: { answers } });
  });

  it("maps cwd, chat, session and resize bodies without replaying a payload", async () => {
    const { transport, calls } = fixture();
    await transport.mux_update_tab_cwd({ tabId: "tab", cwd: "/new" });
    await transport.chat_send_text({ tabId: "tab", text: "hello", clientToken: "once" });
    await transport.rebind_tab_session({ tabId: "tab", sessionId: "bound" });
    await transport.claim_resize_owner({ ...args, cols: 80, rows: 24, force: true });
    expect(calls.map(({ method, path, body }) => ({ method, path, body }))).toEqual([
      { method: "POST", path: "/api/tabs/tab/cwd", body: { cwd: "/new" } },
      { method: "POST", path: "/api/tabs/tab/chat-send", body: { text: "hello", client_token: "once" } },
      { method: "POST", path: "/api/tabs/tab/session/rebind", body: { session_id: "bound" } },
      { method: "POST", path: "/api/tabs/tab/resize-owner", body: { pty_id: "pty", client_id: "client", cols: 80, rows: 24, force: true } },
    ]);
  });

  it("preserves the expected recipient for tracked slash text", async () => {
    const { transport, calls } = fixture();
    const recipient = { provider_id: "fixture", binding_nonce: "own-launch", session_id: "own-session" };
    await transport.chat_send_text({ tabId: "tab", text: "/help", clientToken: "once", expectedRecipient: recipient });
    expect(calls[0].body).toEqual({ text: "/help", client_token: "once", expected_recipient: recipient });
  });

  it("preserves byte and item cursor units on history backfill", async () => {
    const { transport, calls } = fixture();
    await transport.chat_backfill({ tabId:"tab", beforeOffset:4096, offsetKind:"bytes" });
    await transport.chat_backfill({ tabId:"tab", beforeOffset:200, offsetKind:"items" });
    expect(calls.map(call => call.body)).toEqual([
      {before_offset:4096, offset_kind:"bytes"},
      {before_offset:200, offset_kind:"items"},
    ]);
  });

  it("unwraps raw ANSI history, text, spawn id and enabled envelopes", async () => {
    const { transport, calls } = fixture([{ output: "\x1b[31mred" }, { text: "screen" }, { pty_id: "live" }, { enabled: true }]);
    expect(await transport.get_tab_history_raw({ tabId: "a/b" })).toBe("\x1b[31mred");
    expect(await transport.get_tab_text({ tabId: "tab", formatted: true })).toBe("screen");
    expect(await transport.spawn_pty({ cols: 80, rows: 24, predictedId: "live", tabId: "tab" })).toBe("live");
    expect(await transport.mux_set_permission_auto_confirm({ tabId: "tab", enabled: true })).toBe(true);
    expect(calls[0].path).toBe("/api/tabs/a%2Fb/history");
  });

  it("preserves the atomic spawn result shape using the authoritative HTTP snapshot", async () => {
    const snapshot = { workspaces: [], generation: 10 };
    const { transport, calls } = fixture([{ tab_id: "stable", workspace_id: "ws" }, snapshot]);
    expect(await transport.mux_spawn_tab({ params: { agentId: "capability-provider", workspaceId: "ws", reasoningEffort: "high", accountId: "work" } })).toEqual({ tab_id: "stable", workspace_id: "ws", snapshot });
    expect(calls.map(({ method, path }) => [method, path])).toEqual([["POST", "/api/agents/spawn"], ["GET", "/api/mux/state"]]);
    expect(calls[0].body).not.toHaveProperty("activate");
    expect(calls[0].body).toHaveProperty("account_id", "work");
  });

  it("retains long-poll byte cursors for explicit snapshot consumers", async () => {
    const { transport, calls } = fixture([{ data: [0xff], offset: 9 }, { data: [], offset: 9 }]);
    expect(await transport.read_pty({ ptyId: "pty" })).toEqual([0xff]);
    await transport.read_pty({ ptyId: "pty" });
    expect(calls.map(({ path }) => path)).toEqual(["/api/pty/pty/read?offset=0", "/api/pty/pty/read?offset=9"]);
  });

  it("shares reset and truncation cursors with bridge consumers", async () => {
    const offsets = new Map<string, number>([['pty', 8]]);
    const truncations = new Map<string, number>();
    const paths: string[] = [];
    const transport = createTabsTransport({ json: async <T>(_method: HttpMethod, path: string) => {
      paths.push(path);
      return { data: [], offset: 12, truncated_from: 10 } as T;
    } }, { offsets, truncations });
    await transport.read_pty({ ptyId: 'pty' });
    expect(offsets.get('pty')).toBe(12);
    expect(truncations.get('pty')).toBe(10);
    offsets.delete('pty');
    await transport.read_pty({ ptyId: 'pty' });
    expect(paths).toEqual(['/api/pty/pty/read?offset=8', '/api/pty/pty/read?offset=0']);
  });

  it("does not resize without an owner and uses raw escape only for PTY interrupt", async () => {
    const { transport, calls } = fixture();
    expect(await transport.resize_pty({ ptyId: "pty", cols: 80, rows: 24 })).toEqual({ ok: true, ignored: true });
    expect(calls).toHaveLength(0);
    await transport.interrupt_agent({ ptyId: "pty" });
    expect(calls[0].body).toEqual({ data: "\x1b", raw: true });
  });

  it("does not change HTTP failure status/reason or retry uncertain sends", async () => {
    const failure = Object.assign(new Error("session_binding"), { status: 409 });
    const { transport, calls } = fixture([failure]);
    await expect(transport.chat_send_text({ tabId: "tab", text: "one" })).rejects.toBe(failure);
    expect(calls).toHaveLength(1);
  });

  it("passes cancellation through the client", async () => {
    const controller = new AbortController();
    const failure = new DOMException("cancelled", "AbortError");
    controller.abort(failure);
    const { transport, calls } = fixture();
    await expect(transport.chat_start({ tabId: "tab", signal: controller.signal })).rejects.toBe(failure);
    expect(calls[0].opts).toMatchObject({ signal: controller.signal });
  });

  it("refuses a mismatched, empty or malformed remote target before making a request", async () => {
    const { transport, calls } = fixture();
    for (const ordinaryTabId of ["mesh:other/tab", "mesh:peer/", "mesh:peer/tab/extra", "mesh:peer/tab?query", "mesh:peer/tab whitespace"]) {
      await expect(transport.mesh_remote_tab_action({ peerId: "peer", ordinaryTabId, method: "POST", suffix: "/stop" })).rejects.toThrow("owner mismatch");
    }
    expect(calls).toHaveLength(0);
  });

  it("preserves remote prompt conflict responses without sending twice", async () => {
    const prompt = { ok: false, reason: "prompt_not_on_screen" };
    const { transport, calls } = fixture([{ ok: false, status: 409, data: prompt }]);
    expect(await transport.mesh_remote_tab_action({ peerId: "peer", ordinaryTabId: "mesh:peer/tab", method: "POST", suffix: "/prompt/respond", body: {} })).toEqual(prompt);
    expect(calls).toHaveLength(1);
  });

  it("preserves B's recovery results and projects a Chat send receipt once", async () => {
    const tabId = "mesh:peer/tab";
    const resume = { kind: "resumed", deltas: [{
      cursor: "B-next", source_session_id: "B-session", session_binding_nonce: "B-nonce",
      start_offset: 7, offset: 9, messages: [],
    }], caught_up: true };
    const admissions = [{ correlationId: "original-token", evidence: "present" }];
    const status = { tab_id: "tab", send_id: "same-B-send", correlationId: "original-token",
      ledger_id: "B-ledger", sequence: 3, state: "written" };
    const { transport, calls } = fixture([
      ...[resume, admissions, [status]].map(data => ({ ok: true, status: 200, data })),
      { ok: false, status: 503, error: "proof unavailable", data: { error: { code: "admission_proof_unavailable" } } },
    ]);
    const dispatch = (command: string, args = {}) =>
      transport.mesh_remote_tab_action({ ...remoteTransportCall(command, { tabId, ...args })! });
    expect(await dispatch("chat_resume", { cursor: "B-current" })).toEqual(resume);
    expect(await dispatch("chat_send_admissions", { clientTokens: ["original-token"] })).toEqual(admissions);
    expect(await dispatch("chat_send_status")).toEqual([{ ...status, tab_id: tabId }]);
    await expect(dispatch("chat_send_admissions", { clientTokens: ["original-token"] })).rejects.toMatchObject({
      status: 503, code: "admission_proof_unavailable", message: "proof unavailable",
    });
    expect(calls.map(call => call.body)).toEqual([
      { target_instance_id: "peer", method: "POST", path: "/api/tabs/tab/chat-raw/resume", body: { cursor: "B-current" } },
      { target_instance_id: "peer", method: "POST", path: "/api/tabs/tab/chat-send/admissions", body: { clientTokens: ["original-token"] } },
      { target_instance_id: "peer", method: "GET", path: "/api/tabs/tab/chat-send", body: null },
      { target_instance_id: "peer", method: "POST", path: "/api/tabs/tab/chat-send/admissions", body: { clientTokens: ["original-token"] } },
    ]);
  });
});

describe("provider restart transport", () => {
  it("carries the requested provider in the atomic restart body", async () => {
    const { calls, transport } = fixture();
    await transport.restart_agent_tab({ tabId: "main", providerId: "opaque-provider" });
    expect(calls).toHaveLength(1);
    expect(calls[0]).toMatchObject({ method: "POST", path: "/api/tabs/main/agent/restart",
      body: { fresh: false, provider: "opaque-provider" } });
  });
});


it("carries explicit Fast mode on and off through the model Apply request", async () => {
  for (const serviceTier of [{id:null},{id:"priority"},{id:"ultrafast"}]) {
    const { transport, calls } = fixture();
    await transport.switch_agent_model({ tabId: "owned", model: "selected", reasoningEffort: "high", serviceTier });
    expect(calls[0]).toMatchObject({ method: "POST", path: "/api/tabs/owned/agent/model",
      body: { model: "selected", reasoning_effort: "high", service_tier: serviceTier } });
  }
});

it("transports provider handoff selection and explicit stop without losing the tab", async () => {
  const {transport, calls} = fixture();
  await transport.switch_agent_provider({tabId: "owned", providerId: "target", model: "model-token", reasoningEffort: "effort-token", force: true});
  expect(calls).toMatchObject([{method: "POST", path: "/api/tabs/owned/agent/switch-provider", body: {provider: "target", model: "model-token", reasoning_effort: "effort-token", force: true}}]);
});
