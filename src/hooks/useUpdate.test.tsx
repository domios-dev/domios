import { nativeBridge } from "../lib/nativeBridge";
import assert from "node:assert/strict";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../lib/bridge";
import { defaultConfig } from "./useConfig";
import { UpdatePanelProvider, useUpdatePanel } from "./useUpdatePanel";
import {
  normalizeInstallResponse,
  observeInstallPhase,
  UpdateProvider,
  useUpdate,
  type UpdateInfo,
} from "./useUpdate";

const testWindow = new Window({ url: "http://localhost/" });
Object.assign(globalThis, {
  window: testWindow,
  document: testWindow.document,
  Element: testWindow.Element,
  HTMLElement: testWindow.HTMLElement,
  Node: testWindow.Node,
  SVGElement: testWindow.SVGElement,
  IS_REACT_ACT_ENVIRONMENT: true,
  localStorage: testWindow.localStorage,
});

const updateInfo: UpdateInfo = {
  current_version: "1.0.0",
  latest_version: "1.1.0",
  update_available: true,
};

function runtimeStatus(phase: "idle" | "checking" | "downloading" | "applying" | "success" | "error") {
  return {
    phase,
    last_error: null,
    channel: "stable",
    install_method: "unknown",
    current_version: "1.0.0",
    latest_known: "1.1.0",
    update_available: true,
    phase_age_secs: 0,
    downloaded_bytes: 0,
    download_total_bytes: null,
    download_progress: null,
  };
}

type UpdateState = ReturnType<typeof useUpdate>;
let latest: UpdateState | null = null;
let panel: ReturnType<typeof useUpdatePanel> | null = null;
function panelState() {
  if (!panel) throw new Error("update panel did not render");
  return panel;
}

function Harness() {
  latest = useUpdate();
  panel = useUpdatePanel();
  return null;
}

function state(): UpdateState {
  if (!latest) throw new Error("the update provider did not render");
  return latest;
}

async function mount() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(createElement(UpdateProvider, null, createElement(UpdatePanelProvider, null, createElement(Harness))));
  });
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
  return async () => {
    await act(async () => root.unmount());
    host.remove();
    latest = null;
  };
}

async function main() {
  let resolveInstall!: (value: unknown) => void;
  const installResponse = new Promise<unknown>((resolve) => {
    resolveInstall = resolve;
  });
  const previousFetch = globalThis.fetch;
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: async () => ({ ok: false, status: 404 }),
  });

  let restoredPhase: "idle" | "error" = "error";
  let installCalls = 0;
  nativeBridge.updater = {
    check: async () => updateInfo,
    status: async () => ({ ...runtimeStatus(restoredPhase), last_error: restoredPhase === "error" ? "opaque recovered failure" : null }),
    install: async () => { installCalls++; return await installResponse as { status: string }; },
  };
  const currentAvailability = { items: [{ id: "app", kind: "app", installed: "1.0.0", latest: "1.0.0",
    compatible: true, state: "up_to_date", reason: "same_version", channel: null }], errors: {} };
  let checkAvailability = async () => currentAvailability;
  const events = new Map<string, (payload: unknown) => void>();
  __setListenForTest(async <T,>(name: string, handler: (event: { payload: T }) => void) => {
    events.set(name, payload => handler({ payload: payload as T }));
    return () => { events.delete(name); };
  });
  __setInvokeForTest(async <T,>(command: string) => {
    switch (command) {
      case "check_update_availability":
        return await checkAvailability() as T;
      case "get_config":
        return defaultConfig() as T;
      case "check_for_update":
        return updateInfo as T;
      case "get_update_status":
        return runtimeStatus("idle") as T;
      case "install_app_update":
        return installResponse.then((value) => value as T);
      default:
        throw new Error(`unexpected command: ${command}`);
    }
  });

  try {
    const unmount = await mount();
    assert.equal(state().status, "error");
    assert.equal(state().error, "opaque recovered failure");
    assert.equal(state().toastOpen, true);
    await act(async () => { await state().refresh(false, { silent: true }); });
    assert.equal(state().error, "opaque recovered failure");
    assert.equal(state().status, "error");
    assert.equal(installCalls, 0);
    restoredPhase = "idle";
    const firstApply = state().applyUpdate();
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    assert.equal(state().installing, true);
    assert.equal(state().toastOpen, true);
    assert.equal(state().blocking, false);
    assert.equal(state().status, "checking");

    resolveInstall({ status: "already_in_flight", message: "opaque" });
    await act(async () => { assert.equal(await firstApply, null); });
    assert.equal(state().installing, false);
    assert.equal(state().error, "opaque");
    assert.equal(state().toastOpen, true);

    await act(async () => events.get("update-status-changed")?.({
      ...runtimeStatus("downloading"), downloaded_bytes: 25, download_total_bytes: 100,
    }));
    assert.equal(state().status, "downloading");
    assert.equal(state().blocking, false);
    assert.equal(state().downloadedBytes, 25);
    await act(async () => state().dismissUpdateToast());
    assert.equal(state().toastOpen, false);
    await act(async () => events.get("update-status-changed")?.(runtimeStatus("downloading")));
    assert.equal(state().toastOpen, false);
    await act(async () => events.get("update-status-changed")?.(runtimeStatus("applying")));
    assert.equal(state().blocking, true);
    await act(async () => events.get("update-status-changed")?.({ ...runtimeStatus("error"), last_error: "opaque preparation failure" }));
    assert.equal(state().blocking, false);
    assert.equal(state().toastOpen, true);
    await act(async () => events.get("update-status-changed")?.(runtimeStatus("idle")));
    assert.equal(state().installing, false);

    let observation = observeInstallPhase("checking", false, false);
    observation = observeInstallPhase("idle", observation.activePhaseObserved, false);
    assert.equal(observation.finished, true);
    assert.equal(observation.keepVisible, false);

    const returned = observeInstallPhase("idle", false, true);
    assert.equal(returned.finished, true);

    await unmount();
    nativeBridge.updater = {
      check: async () => updateInfo,
      status: async () => runtimeStatus("idle"),
      install: async () => { throw new Error("opaque refusal"); },
    };
    const unmountFailure = await mount();
    nativeBridge.updater.check = async () => { throw new Error("offline"); };
    await act(async () => { await state().refresh(true, { silent: true }); });
    assert.equal(state().error, null);
    // A failed explicit check is no update: no error, and the status returns to what was last known.
    const known = state().status;
    await act(async () => { await state().refresh(true); });
    assert.equal(state().error, null);
    assert.equal(state().status, known);
    nativeBridge.updater.check = async () => updateInfo;
    let finishCheck!: (value: UpdateInfo) => void;
    nativeBridge.updater.check = () => new Promise(resolve => { finishCheck = resolve; });
    let discovery!: Promise<void>;
    await act(async () => { discovery = state().refresh(true, { silent: true }); });
    await act(async () => events.get("update-status-changed")?.({ ...runtimeStatus("error"), last_error: "opaque durable failure" }));
    await act(async () => { finishCheck({ ...updateInfo, update_available: false }); await discovery; });
    await act(async () => events.get("update-status-changed")?.(runtimeStatus("idle")));
    assert.equal(state().status, "error");
    assert.equal(state().error, "opaque durable failure");
    assert.equal(state().installing, false);
    nativeBridge.updater.check = async () => updateInfo;
    await act(async () => { await state().applyUpdate(); });
    assert.equal(state().status, "error");
    assert.equal(state().toastOpen, true);
    await act(async () => { await state().applyUpdate({ failureSurface: "inline" }); });
    assert.equal(state().toastOpen, false);
    nativeBridge.updater.check = async () => ({ ...updateInfo, latest_version: updateInfo.current_version, update_available: false });
    await act(async () => { await state().refresh(true); });
    assert.equal(state().error, null);
    assert.equal(state().status, "up-to-date");
    assert.equal(state().phase, "idle");
    assert.equal(state().toastOpen, false);
    await act(async () => events.get("update-status-changed")?.({
      ...runtimeStatus("error"), last_error: "E1", error_revision: "native-E1",
    }));
    let finishAvailability!: (value: typeof currentAvailability) => void;
    checkAvailability = () => new Promise(resolve => { finishAvailability = resolve; });
    let retiredNativeRevision: string | null | undefined;
    nativeBridge.updater.check = async (_force, _channel, revision) => {
      retiredNativeRevision = revision;
      return { ...updateInfo, latest_version: updateInfo.current_version, update_available: false };
    };
    let retry!: Promise<void>;
    await act(async () => { retry = panelState().retry(); });
    await act(async () => events.get("update-status-changed")?.({
      ...runtimeStatus("error"), last_error: "E2", error_revision: "native-E2",
    }));
    await act(async () => { finishAvailability(currentAvailability); await retry; });
    assert.equal(retiredNativeRevision, "native-E1");
    assert.equal(state().error, "E2");
    assert.equal(state().status, "error");
    assert.ok(panelState().error);
    assert.equal(panelState().errorDetails, "E2");
    checkAvailability = async () => {
      events.get("update-status-changed")?.({ ...runtimeStatus("error"), last_error: "E2", error_revision: "native-E2" });
      return currentAvailability;
    };
    await act(async () => { await panelState().retry(); });
    assert.equal(retiredNativeRevision, "native-E2");
    assert.equal(state().error, null);
    assert.equal(Boolean(panelState().error), false);
    for (const [reason, message] of [
      ["operation_conflict", "No installer was started. This operation ID was already used with different update options. Check for updates and start a new update."],
      ["retired_operation", "No installer was started. This operation was already admitted, and its recorded result is no longer retained. Check for updates and start a new update."],
      ["incarnation_limit", "No installer was started. This Domios server has reached its update admission limit. Restart Domios, then check for updates and update again."],
      ["stale_server_incarnation", "No installer was started. This request belongs to a previous Domios server. Check for updates and start a new update."],
    ]) {
      let requests = 0;
      const refusal = JSON.stringify({ kind: "native_update_admission_refused", reason, message });
      nativeBridge.updater.install = async () => { requests++; throw refusal; };
      await act(async () => { assert.equal(await state().applyUpdate({ failureSurface: "inline" }), null); });
      assert.equal(panelState().error, message);
      assert.equal(panelState().errorDetails, refusal);
      assert.equal(requests, 1);
    }
    await unmountFailure();
    checkAvailability = async () => currentAvailability;
    nativeBridge.updater.check = async () => updateInfo;
    nativeBridge.updater.status = async () => runtimeStatus("idle");
    const unmountDiscovery = await mount();
    const checkCompletions: Array<(value: UpdateInfo) => void> = [];
    nativeBridge.updater.check = () => new Promise(resolve => { checkCompletions.push(resolve); });
    let firstCheck!: Promise<void>, secondCheck!: Promise<void>;
    await act(async () => { firstCheck = state().refresh(); secondCheck = state().refresh(); });
    await act(async () => { checkCompletions[1]({ ...updateInfo, latest_version: "3.0.0" }); await secondCheck; });
    await act(async () => { checkCompletions[0](updateInfo); await firstCheck; });
    assert.equal(state().latestVersion, "3.0.0");
    await act(async () => { firstCheck = state().refresh(); });
    await act(async () => events.get("update-available")?.({ ...updateInfo, latest_version: "4.0.0" }));
    await act(async () => { checkCompletions[2](updateInfo); await firstCheck; });
    assert.equal(state().latestVersion, "4.0.0");
    await act(async () => { firstCheck = state().refresh(); });
    await unmountDiscovery();
    checkCompletions[3](updateInfo);
    await firstCheck;
  } finally {
    nativeBridge.updater = undefined;
    __setInvokeForTest(null);
    __setListenForTest(null);
    Object.defineProperty(globalThis, "fetch", {
      configurable: true,
      value: previousFetch,
    });
  }

  assert.equal(normalizeInstallResponse({ status: "scheduled" }).status, "updating");
  assert.equal(normalizeInstallResponse({ status: "already_in_flight" }).status, "already_in_flight");
  assert.equal(normalizeInstallResponse({ status: "no_update" }).status, "up_to_date");
  for (const status of ["blocked", "elevation_required", "unknown"]) {
    assert.throws(() => normalizeInstallResponse({ status, message: "opaque refusal" }), /opaque refusal/);
  }
  assert.throws(() => normalizeInstallResponse(null), /Malformed install response/);
  console.log("useUpdate first-poll install contract ok");
}

void main();
