import { defaultConfig, mergeConfigEvent } from "./useConfig";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const memory = defaultConfig().memory;
assert(memory?.extraction_model === "", "extraction model waits for the provider catalog");
assert(memory?.dreaming_model === "", "dreaming model waits for the provider catalog");
assert(memory?.worker_model === "", "worker model waits for the provider catalog");

const supporter = defaultConfig().supporter;
assert(supporter.adapter == null, "Supporter starts without an active connection");
assert(Object.keys(supporter.connections).length === 0, "Supporter starts without saved connections");
assert(!("model" in supporter), "Supporter models belong to individual connections");
assert(!("endpoint" in supporter), "Supporter endpoints belong to individual connections");

const orchestration = defaultConfig().orchestration;
assert(orchestration.plan_reminders, "plan reminders default on");
assert(orchestration.nudge_interval_secs === 1800, "plan reminders default to 1800 seconds");
assert(!(["nudge_after", "turns"].join("_") in orchestration), "turn configuration must not be exposed");
assert(!(["idle", "poke"].join("_") in orchestration), "idle configuration must not be exposed");

const authenticated = defaultConfig();
authenticated.api.token = "runtime-secret";
const redacted = structuredClone(authenticated);
redacted.api.token = null;
assert(
  mergeConfigEvent(authenticated, redacted).api.token === "runtime-secret",
  "a redacted config event cannot erase the desktop runtime token",
);

const rotated = structuredClone(authenticated);
rotated.api.token = "rotated-secret";
assert(
  mergeConfigEvent(authenticated, rotated).api.token === "rotated-secret",
  "an explicit token rotation replaces the previous runtime token",
);

console.log("✓ config defaults and runtime token event merging are stable");

async function nativeConfigSaves() {
  const { act, createElement, useLayoutEffect } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const { Window } = await import("happy-dom");
  const { __setInvokeForTest, __setListenForTest } = await import("../lib/bridge");
  const { useConfig } = await import("./useConfig");
  const { withFailureDeadline } = await import("../test/withFailureDeadline");
  const win = new Window();
  Object.assign(win, { __CODETERM_DAEMON__: { origin: /^https?:/.test(win.location.origin) ? win.location.origin : "http://127.0.0.1:7686", token: "runtime-secret" } });
  const original = new Map<string, PropertyDescriptor | undefined>();
  for (const key of ["window", "document", "navigator", "HTMLElement", "Node"]) {
    original.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value: win[key as keyof typeof win], configurable: true });
  }
  original.set("IS_REACT_ACT_ENVIRONMENT", Object.getOwnPropertyDescriptor(globalThis, "IS_REACT_ACT_ENVIRONMENT"));
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });
  let finishRead!: (config: ReturnType<typeof defaultConfig>) => void;
  const read = new Promise<ReturnType<typeof defaultConfig>>((resolve) => { finishRead = resolve; });
  let stored = structuredClone(authenticated);
  const persisted = () => stored;
  let writes = 0;
  let emit: ((event: { payload: ReturnType<typeof defaultConfig> }) => void) | undefined;
  __setInvokeForTest(async <T>(command: string, args?: Record<string, unknown>) => {
    if (command === "get_config") return await read as T;
    assert(command === "update_config", "only config commands reach the native fixture");
    stored = structuredClone(args?.config as ReturnType<typeof defaultConfig>);
    writes += 1;
    return undefined as T;
  });
  __setListenForTest(async <T>(event: string, handler: (event: { payload: T }) => void) => {
    assert(event === "config-changed", "only the config listener is registered");
    emit = (value) => handler({ payload: value.payload as T });
    return () => { emit = undefined; };
  });
  let current!: ReturnType<typeof useConfig>;
  function Probe() {
    const config = useConfig();
    useLayoutEffect(() => { current = config; });
    return null;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(createElement(Probe)));
    const loadingSave = current.updateConfig;
    let refusal: unknown;
    try { await withFailureDeadline(loadingSave(defaultConfig()), "native loading refusal"); } catch (error) { refusal = error; }
    assert(refusal instanceof Error && refusal.message === "Config is not ready for this peer", "native defaults cannot be saved before readiness");
    assert(writes === 0, "native loading saves never reach persistence");
    await act(async () => finishRead(authenticated));
    assert(emit, "native config listener registered");
    await act(async () => emit!({ payload: redacted }));
    assert(current.config.api.token === "runtime-secret", "real native hook preserves a same-peer redacted token");
    await act(async () => current.updateConfig({ ...current.config, font: { ...current.config.font, size: 18 } }));
    assert(persisted().api.token === "runtime-secret" && persisted().font.size === 18, "native save persists the current peer config and token");
    const { configureDaemonTransport } = await import("../lib/daemonBootstrap");
    configureDaemonTransport({ origin: "http://127.0.0.1:7686", token: "rotated-secret" });
    await act(async () => emit!({ payload: rotated }));
    await act(async () => current.updateConfig({ ...current.config, font: { ...current.config.font, size: 20 } }));
    assert(persisted().api.token === "rotated-secret" && persisted().font.size === 20, "native token rotation survives a valid subsequent save");
    console.log("PASS native config readiness, redacted-token preservation and rotation through the real hook");
  } finally {
    await act(async () => root.unmount());
    finishRead(authenticated);
    __setInvokeForTest(null); __setListenForTest(null);
    host.remove();
    for (const [key, descriptor] of original) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key);
    }
    await win.happyDOM.close();
  }
}

void nativeConfigSaves().catch((error) => { console.error(error); process.exitCode = 1; });
