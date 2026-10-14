import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { AgentRuntimeState, AgentRuntimeStateEvent } from "../../lib/agentRuntimeState";
import { AgentRuntimeBanner } from "./AgentRuntimeBanner";
import { runtimeBannerView } from "../../lib/agentRuntimeState";
import { useAgentRuntimeState } from "./useAgentRuntimeState";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("../../lib/bridge", () => bridge);
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("renders a restored fault and wires one Retry to ensure-loaded, retaining it disabled until recovery", async () => {
  let emit!: (event: { payload: AgentRuntimeStateEvent }) => void;
  let resolve!: (state: AgentRuntimeState) => void;
  const unlisten = vi.fn();
  bridge.listen.mockImplementation(async (_event, handler) => { emit = handler; return unlisten; });
  bridge.invoke.mockImplementation(() => new Promise<AgentRuntimeState>((done) => { resolve = done; }));
  const snapshot: AgentRuntimeState = { state: "degraded", epoch: 2, reason: "fixture fault" };
  function Harness() {
    const runtime = useAgentRuntimeState("owned", "binding", snapshot);
    return <AgentRuntimeBanner state={runtime.state} onRetry={() => { void runtime.retry(); }} />;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    expect(bridge.listen).toHaveBeenCalledWith("agent-runtime-state", expect.any(Function));
    expect(host.querySelectorAll('[role="status"]')).toHaveLength(1);
    expect(host.querySelector('[role="status"]')?.getAttribute("aria-label")).toBeTruthy();
    expect(host.textContent).toContain(runtimeBannerView(snapshot)?.reason);
    const retry = host.querySelector<HTMLButtonElement>('button[aria-label="Retry"]')!;
    expect(retry).not.toBeNull();
    expect(host.querySelectorAll("button")).toHaveLength(1);
    await act(async () => { retry.click(); retry.click(); });
    expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("agent_runtime_retry", { tabId: "owned" });
    expect(retry.disabled).toBe(true);
    expect(host.textContent).toContain(runtimeBannerView({ ...snapshot, state: "loading" })?.reason);
    await act(async () => emit({ payload: { tab_id: "owned", session_binding_nonce: "binding", state: { state: "loaded", epoch: 3 } } }));
    expect(host.querySelector('[role="status"]')).toBeNull();
    await act(async () => resolve({ state: "degraded", epoch: 2, reason: "stale receipt" }));
    expect(host.querySelector('[role="status"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
  }
  expect(unlisten).toHaveBeenCalledTimes(1);
});

it("keeps raw diagnostics inside a closed Details disclosure and wires Retry", async () => {
  const state: AgentRuntimeState = {
    state: "degraded", epoch: 1,
    reason: 'Provider operation failed: HookFailed: Exception { stack: "at eval_script:16:66" }',
  };
  const retry = vi.fn();
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<AgentRuntimeBanner state={state} onRetry={retry} />));
    const details = host.querySelector("details")!;
    expect(details).not.toBeNull();
    expect(details.open).toBe(false);
    expect(details.querySelector("summary")?.textContent).toBe("Details");
    expect(details.querySelector("pre")?.textContent).toBe(state.reason);
    const banner = host.querySelector('[role="status"]')!.cloneNode(true) as HTMLElement;
    banner.querySelector("details")!.remove();
    expect(banner.textContent).toContain(runtimeBannerView(state)?.reason);
    expect(banner.textContent).not.toContain(state.reason);
    await act(async () => { details.querySelector("summary")!.click(); });
    expect(details.open).toBe(true);
    expect(details.querySelector("pre")?.textContent).toBe(runtimeBannerView(state)?.details);
    await act(async () => { details.querySelector("summary")!.click(); });
    expect(details.open).toBe(false);
    await act(async () => { host.querySelector<HTMLButtonElement>('button[aria-label="Retry"]')!.click(); });
    expect(retry).toHaveBeenCalledTimes(1);
  } finally {
    await act(async () => root.unmount());
  }
});
