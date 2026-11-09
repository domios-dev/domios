import assert from "node:assert/strict";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../lib/bridge";
import { type UpdateAvailabilitySnapshot } from "../lib/updateAvailability";
import { useUpdateAvailability } from "./useUpdateAvailability";

const testWindow = new Window({ url: "http://localhost/" });
Object.assign(globalThis, {
  window: testWindow, document: testWindow.document,
  Element: testWindow.Element, HTMLElement: testWindow.HTMLElement,
  Node: testWindow.Node, IS_REACT_ACT_ENVIRONMENT: true,
});

function snapshot(version: string, errors: Record<string, string> = {}): UpdateAvailabilitySnapshot {
  return { items: [{ id: "app", kind: "app", installed: "1.0.0", latest: version,
    compatible: true, state: version === "1.0.0" ? "up_to_date" : "update_available",
    reason: "fixture", channel: "official" }], errors };
}

let state: ReturnType<typeof useUpdateAvailability>;
function Harness() { state = useUpdateAvailability(); return null; }

async function main() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const calls: Array<{ command: string; force: unknown }> = [];
  let handler: ((snapshot: UpdateAvailabilitySnapshot) => void) | undefined;
  let stops = 0;
  let response: () => Promise<UpdateAvailabilitySnapshot> = async () => snapshot("1.0.0");
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    calls.push({ command, force: args?.force });
    assert.equal(command, "check_update_availability", "discovery never installs or restarts");
    return await response() as T;
  });
  __setListenForTest(async <T,>(name: string, callback: (event: { payload: T }) => void) => {
    assert.equal(name, "update-availability-changed");
    handler = value => callback({ payload: value as T });
    return () => { stops++; handler = undefined; };
  });
  try {
    await act(async () => { root.render(createElement(Harness)); });
    assert.equal(state!.ready, true);
    assert.equal(state!.snapshot.items[0].latest, "1.0.0");
    response = async () => snapshot("2.0.0", { app: "retained diagnostic" });
    const initialCalls = calls.length;
    for (const name of ["focus", "online", "pageshow"]) {
      await act(async () => { testWindow.dispatchEvent(new testWindow.Event(name)); });
    }
    Object.defineProperty(document, "hidden", { configurable: true, value: false });
    for (const name of ["visibilitychange", "resume"]) {
      await act(async () => { document.dispatchEvent(new testWindow.Event(name) as unknown as Event); });
    }
    assert.equal(calls.length - initialCalls, 5, "all lifecycle signals reach the daemon policy owner");
    assert.equal(state!.snapshot.items[0].latest, "2.0.0");
    assert.equal(state!.snapshot.errors.app, "retained diagnostic");
    assert(calls.every(call => call.force === false));
    Object.defineProperty(document, "hidden", { configurable: true, value: true });
    const beforeHidden = calls.length;
    document.dispatchEvent(new testWindow.Event("visibilitychange") as unknown as Event);
    assert.equal(calls.length, beforeHidden);

    let finish!: (value: UpdateAvailabilitySnapshot) => void;
    response = () => new Promise(resolve => { finish = resolve; });
    let pending!: Promise<UpdateAvailabilitySnapshot>;
    await act(async () => { pending = state!.refresh(); });
    await act(async () => { handler!(snapshot("3.0.0")); });
    await act(async () => { finish(snapshot("1.0.0")); await pending; });
    assert.equal(state!.snapshot.items[0].latest, "3.0.0", "late response cannot overwrite a published inventory");

    const completions: Array<(value: UpdateAvailabilitySnapshot) => void> = [];
    response = () => new Promise(resolve => { completions.push(resolve); });
    let older!: Promise<UpdateAvailabilitySnapshot>, newer!: Promise<UpdateAvailabilitySnapshot>;
    await act(async () => { older = state!.refresh(); newer = state!.refresh(); });
    await act(async () => { completions[1](snapshot("4.0.0")); await newer; });
    await act(async () => { completions[0](snapshot("2.0.0")); await older; });
    assert.equal(state!.snapshot.items[0].latest, "4.0.0", "latest request owns state");

    response = async () => { throw new Error("offline"); };
    await act(async () => { testWindow.dispatchEvent(new testWindow.Event("focus")); });
    assert.equal(state!.snapshot.items[0].latest, "4.0.0", "offline discovery preserves inventory");
    response = async () => snapshot("5.0.0");
    await act(async () => { testWindow.dispatchEvent(new testWindow.Event("online")); });
    assert.equal(state!.snapshot.items[0].latest, "5.0.0");

    response = () => new Promise(resolve => { finish = resolve; });
    await act(async () => { pending = state!.refresh(); });
    await act(async () => { root.unmount(); });
    const beforeUnmountedSignals = calls.length;
    for (const name of ["focus", "online", "pageshow"]) testWindow.dispatchEvent(new testWindow.Event(name));
    for (const name of ["resume", "visibilitychange"]) document.dispatchEvent(new testWindow.Event(name) as unknown as Event);
    assert.equal(calls.length, beforeUnmountedSignals, "unmount removes lifecycle signals");
    finish(snapshot("6.0.0"));
    await pending;
    assert.equal(state!.snapshot.items[0].latest, "5.0.0");
    assert.equal(stops, 1);
    console.log("✓ discovery lifecycle signals, offline recovery, stale results and cleanup");
  } finally {
    __setInvokeForTest(null);
    __setListenForTest(null);
    host.remove();
    await testWindow.happyDOM.close();
  }
}

void main();
