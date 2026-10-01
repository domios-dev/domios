import assert from "node:assert/strict";
import { act } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { ApiConfig } from "../../lib/types";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";
import { withFailureDeadline } from "../../test/withFailureDeadline";

const renderAct = (body: () => Promise<void>) => withFailureDeadline(act(body), "plan provider render");

async function main() {
  const win = new Window({ url: "http://gateway:7685" });
  const globals = {
    window: win, document: win.document, navigator: win.navigator,
    localStorage: win.localStorage, sessionStorage: win.sessionStorage,
    IS_REACT_ACT_ENVIRONMENT: true,
  };
  for (const [name, value] of Object.entries(globals)) {
    Object.defineProperty(globalThis, name, { value, configurable: true, writable: true });
  }
  const { __setListenForTest } = await import("../../lib/bridge");
  const { setRelayTarget, clearRelayTarget, invalidateWebRequestContext } = await import("../../lib/webApi");
  const { PlansProvider, usePlanIndex, useFullPlans } = await import("./PlansProvider");
  const API: ApiConfig = { enabled: true, host: "gateway", port: 7685, token: "token", cors: true };
  type Handler = (event: { payload: unknown }) => void;
  const handlers = new Map<string, Set<Handler>>();
  __setListenForTest(async (name, handler) => {
    const listeners = handlers.get(name) ?? new Set<Handler>();
    const listener = handler as Handler;
    listeners.add(listener);
    handlers.set(name, listeners);
    return () => { listeners.delete(listener); };
  });

  let revision = 0;
  let holdReads = false;
  const heldReads: Array<() => void> = [];
  const reads: string[] = [];
  globalThis.fetch = async (input, init) => {
    const url = new URL(String(input));
    const envelope = url.pathname === "/api/mesh/relay-to"
      ? JSON.parse(String(init?.body)) as { target_peer_id: string; path: string }
      : null;
    const machine = envelope?.target_peer_id ?? "self";
    const path = envelope?.path ?? `${url.pathname}${url.search}`;
    reads.push(`${machine}:${path}`);
    const plan = { ...FIXTURE_PLAN, id: `pl_${machine}`, title: `${machine}-${revision}` };
    const data = path === "/api/plans"
      ? { plans: [{ plan_id: plan.id, title: plan.title }] }
      : path.startsWith("/api/plans?") ? { plans: [plan] } : plan;
    const response = () => Response.json(envelope ? { ok: true, status: 200, data } : data);
    if (holdReads) return new Promise<Response>((resolve) => { heldReads.push(() => resolve(response())); });
    return response();
  };

  let snapshot = { index: [] as string[], full: [] as string[], titles: [] as string[] };
  function Probe() {
    const { index } = usePlanIndex();
    const { plans } = useFullPlans();
    snapshot = { index: index.map((entry) => entry.plan_id), full: plans.map((plan) => plan.id), titles: plans.map((plan) => plan.title) };
    return null;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  try {
    await renderAct(async () => { root.render(<PlansProvider api={API}><Probe /></PlansProvider>); });
    assert.deepEqual(snapshot.full, ["pl_self"]);

    async function switchPeer(peer: string | null, switchId: number) {
      await renderAct(async () => {
        invalidateWebRequestContext();
        const beforeReset = reads.length;
        flushSync(() => { win.dispatchEvent(new win.CustomEvent("codeterm-peer-switch-reset", { detail: { switchId } })); });
        assert.deepEqual(snapshot.full, [], "old full plans disappear before peer rebinding");
        assert.deepEqual(snapshot.index, [], "old index disappears before peer rebinding");
        assert.equal(reads.length, beforeReset, "disabled queries cannot reload the old peer during reset");
        if (peer) setRelayTarget(peer, peer);
        else clearRelayTarget();
        invalidateWebRequestContext();
        win.dispatchEvent(new win.CustomEvent("codeterm-peer-switch-rebind", { detail: { switchId } }));
      });
    }

    await switchPeer("ubunta", 1);
    assert.deepEqual(snapshot.full, ["pl_ubunta"]);
    assert.deepEqual(snapshot.index, ["pl_ubunta"]);
    revision = 1;
    await renderAct(async () => {
      for (const handler of handlers.get("plan-changed") ?? []) handler({ payload: { plan_id: "pl_ubunta" } });
    });
    assert.deepEqual(snapshot.titles, ["ubunta-1"]);

    holdReads = true;
    await renderAct(async () => {
      for (const handler of handlers.get("plan-changed") ?? []) handler({ payload: { plan_id: "pl_ubunta" } });
    });
    assert(heldReads.length > 0);
    holdReads = false;
    await switchPeer("eight", 2);
    assert.deepEqual(snapshot.full, ["pl_eight"]);
    await renderAct(async () => { heldReads.splice(0).forEach((resolve) => resolve()); });
    assert.deepEqual(snapshot.full, ["pl_eight"], "late peer reads cannot contaminate the current machine");

    await switchPeer(null, 3);
    assert.deepEqual(snapshot.full, ["pl_self"]);
    assert.deepEqual(snapshot.index, ["pl_self"]);
    assert(reads.includes("ubunta:/api/plans?full=1"));
    assert(reads.includes("eight:/api/plans?full=1"));
  } finally {
    await renderAct(async () => {
      root.unmount();
      heldReads.splice(0).forEach((resolve) => resolve());
    });
    host.remove();
    __setListenForTest(null);
  }
  assert([...handlers.values()].every((listeners) => listeners.size === 0));
  console.log("PlansProvider peer switching passed");
}

void main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
