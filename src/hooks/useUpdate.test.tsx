import assert from "node:assert/strict";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../lib/bridge";
import { defaultConfig } from "./useConfig";
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

function Harness() {
  latest = useUpdate();
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
    root.render(createElement(UpdateProvider, null, createElement(Harness)));
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

  __setListenForTest(async () => () => {});
  __setInvokeForTest(async <T,>(command: string) => {
    switch (command) {
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
    const firstApply = state().applyUpdate();
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    assert.equal(state().installing, true);
    assert.equal(state().toastOpen, true);
    assert.equal(state().status, "checking");

    resolveInstall({ status: "already_in_flight", message: "opaque" });
    assert.equal((await firstApply)?.status, "already_in_flight");
    assert.equal(state().installing, true);
    assert.equal(state().toastOpen, true);
    assert.equal((await state().applyUpdate())?.status, "already_in_flight");

    let observation = observeInstallPhase("checking", false, false);
    observation = observeInstallPhase("idle", observation.activePhaseObserved, false);
    assert.equal(observation.finished, true);
    assert.equal(observation.keepVisible, false);

    const returned = observeInstallPhase("idle", false, true);
    assert.equal(returned.finished, true);

    await unmount();
  } finally {
    __setInvokeForTest(null);
    __setListenForTest(null);
    Object.defineProperty(globalThis, "fetch", {
      configurable: true,
      value: previousFetch,
    });
  }

  assert.equal(normalizeInstallResponse({ status: "already_in_flight" }).status, "already_in_flight");
  console.log("useUpdate first-poll install contract ok");
}

void main();
