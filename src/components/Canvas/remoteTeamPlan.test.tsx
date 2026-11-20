import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { peerEventRail } from "../../lib/peerEventRail";
import type { ApiConfig } from "../../lib/types";
import { setRelayTarget } from "../../lib/webApi";
import { canvasFocusStore } from "./canvasFocusStore";
import { RemoteTeamPlanDrawer } from "./RemoteTeamPlanDrawer";
import { readRemoteTeamPlan, runRemotePlanAction } from "./remoteTeamPlanModel";

const ports = vi.hoisted(() => ({ fetch: vi.fn<typeof fetch>(), subscribe: vi.fn() }));
vi.mock("../../lib/daemonBootstrap", async importOriginal => {
  const actual = await importOriginal<typeof import("../../lib/daemonBootstrap")>();
  const { createTransportClient } = await import("../../lib/transportClient");
  const local = createTransportClient({ origin: "http://viewer", token: "viewer-token", fetch: ports.fetch });
  return {
    ...actual,
    baseDaemonTransport: () => ({ ...local, subscribe: ports.subscribe }),
    daemonTransportForPeer: (peerId?: string) => createTransportClient({
      origin: "http://viewer", token: "viewer-token", relayPeerId: peerId, fetch: ports.fetch,
    }),
  };
});

const api: ApiConfig = {
  enabled: true, host: "viewer", port: 7685, token: "viewer-token", cors: false,
  mesh_tailnet_host: null, cors_allow_origins: [], preview_secret_paths: [],
};
const target = { peerId: "owner-B", teamId: "team-B", name: "Remote team" };
const reason = "The owning machine requires Full mesh access for review and dispatch.";
const grant = { protocol_version: null, unavailable_reason: reason };
const state = {
  team_id: "team-B", team: { id: "team-B", workspace_id: "ws-B" },
  active_plan: { id: "plan-B", team_id: "team-B", title: "Remote Plan" },
  remote_plan_actions: grant,
};
const relay = (data: unknown, status = 200) => Response.json({ ok: status === 200, status, data });

afterEach(() => {
  ports.fetch.mockReset();
  ports.subscribe.mockReset();
  canvasFocusStore.closeWorkspace();
  localStorage.clear();
});

it("looks up the Team on its exact owning installation despite a different selected peer", async () => {
  setRelayTarget("elsewhere", "Other machine");
  ports.fetch.mockResolvedValueOnce(relay(state));
  expect((await readRemoteTeamPlan(api, target)).active_plan?.id).toBe("plan-B");
  const [url, init] = ports.fetch.mock.calls[0];
  expect(url).toBe("http://viewer/api/mesh/relay-to");
  expect(new Headers(init?.headers).get("Authorization")).toBe("Bearer viewer-token");
  expect(JSON.parse(String(init?.body))).toEqual({
    method: "GET", target_instance_id: "owner-B", path: "/api/teams/team-B/mission-control/state?limit=1", body: null,
  });
  ports.fetch.mockResolvedValueOnce(relay({ ...state, active_plan: { ...state.active_plan, team_id: "another-team" } }));
  await expect(readRemoteTeamPlan(api, target)).rejects.toThrow("did not match the selected Team");
});

it("reports both the published read-only grant and a live dispatch refusal", async () => {
  const dispatch = (actions: typeof state.remote_plan_actions | { protocol_version: number; unavailable_reason: null }) =>
    runRemotePlanAction(target, actions, "plan-B", "task-B", "plan.task.dispatch", {
      execution_intent: "read_only", workspace_id: "ws-B",
    });
  await expect(dispatch(grant)).rejects.toThrow(reason);
  expect(ports.fetch).not.toHaveBeenCalled();
  await expect(dispatch({ protocol_version: 0, unavailable_reason: null })).rejects.toThrow("has not confirmed permission");
  expect(ports.fetch).not.toHaveBeenCalled();
  ports.fetch.mockResolvedValueOnce(relay({ message: "Peer grant was revoked" }, 403));
  await expect(dispatch({ protocol_version: 14, unavailable_reason: null })).rejects.toThrow("Peer grant was revoked");
  expect(JSON.parse(String(ports.fetch.mock.calls[0][1]?.body))).toMatchObject({
    method: "POST", target_instance_id: "owner-B", path: "/api/mesh/rpc",
    body: { method: "plan.task.dispatch", protocol_version: 14, params: { plan_id: "plan-B", task_id: "task-B", workspace_id: "ws-B" } },
  });
  expect(ports.fetch).toHaveBeenCalledTimes(1);
  ports.fetch.mockResolvedValueOnce(relay({ result: { state: "awaiting_dispatch_ready" } }));
  await expect(dispatch({ protocol_version: 14, unavailable_reason: null })).resolves.toBeUndefined();
});

it("refreshes the open Team drawer on the owner's Plan rail and preserves the target when opening the canvas", async () => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, "IS_REACT_ACT_ENVIRONMENT");
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const receivers = new Map<string, (name: string, frame: unknown) => void>();
  ports.subscribe.mockImplementation((path: string, receive: (name: string, frame: unknown) => void) => {
    const peer = new URL(path, "http://viewer").searchParams.get("peer")!;
    receivers.set(peer, receive);
    return () => receivers.delete(peer);
  });
  const stops = ["owner-B", "another-peer"].map(peer => peerEventRail.subscribe(peer, "test-link", () => {}));
  const listPath = "/api/plans/plan-B/projection";
  let revision = "1";
  let status = "pending";
  const planReads: string[] = [];
  ports.fetch.mockImplementation(async (_url, init) => {
    const request = JSON.parse(String(init?.body));
    expect(request.target_instance_id).toBe("owner-B");
    if (request.path.startsWith(listPath)) {
      planReads.push(revision);
      return relay({
        id: "plan-B", revision, task_window: { next_cursor: null },
        tasks: [{ id: "task-B", key: "B-1", name: "Remote task", status }],
      });
    }
    return relay(state);
  });
  const send = (peer: string, event: string, seq: number, data?: unknown) =>
    receivers.get(peer)!("message", { boot: "boot", seq, event_type: event, data });
  const Drawer = () => <RemoteTeamPlanDrawer api={api} target={{ planId: "", remoteTeam: target }}
    state="open" panelRef={useRef<HTMLElement | null>(null)} onClose={() => {}} />;
  try {
    for (const peer of receivers.keys()) send(peer, "peer-replay-complete", 1);
    await act(async () => root.render(<Drawer />));
    expect(host.textContent).toContain("Remote taskpending");
    expect(host.textContent).toContain(reason);
    revision = "2";
    status = "in_progress";
    const event = { plan_id: "plan-B", revision, previous_revision: "1", reset: false, deleted: false, task_ids: ["task-B"] };
    await act(async () => {
      send("another-peer", "plan-changed", 2, event);
      send("owner-B", "plan-changed", 2, { ...event, plan_id: "unrelated" });
    });
    expect(planReads).toEqual(["1"]);
    await act(async () => send("owner-B", "plan-changed", 3, event));
    expect(planReads).toEqual(["1", "2"]);
    expect(host.textContent).toContain("Remote taskin progress");
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Open Plan canvas"]')!.click());
    expect(canvasFocusStore.getWorkspaceTarget()).toEqual({
      remoteTeam: target, planId: "plan-B", jump: { view: "board" },
    });
  } finally {
    await act(async () => root.unmount());
    host.remove();
    for (const stop of stops) stop();
    if (previous) Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", previous);
    else Reflect.deleteProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT");
  }
});
