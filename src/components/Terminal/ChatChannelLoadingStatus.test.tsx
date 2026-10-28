import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { runtimeActivityLabel } from "../../lib/agentRuntimeTiming";
import { tabActivity } from "../../lib/tabActivity";
import { AgentStateIndicator } from "./AgentStateIndicator";
import { chatChannelLoadingRow } from "./chatBodyVisibility";
import { runtimeReduce } from "../../../plugins/provider-codex/src/runtimeReduce";
import type { JsonValue, RuntimeContext, RuntimeFrame } from "@codeterm/plugin-sdk";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("hosts the accessible waiting dots in the existing model row without an extra row", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const label = "Fixture channel pending";
  const chooseModel = vi.fn();
  const render = async (loading: boolean) => {
    const row = chatChannelLoadingRow(loading, { kind: "none" }, false);
    await act(async () => root.render(
      <AgentStateIndicator
        activity={tabActivity({ agent_state: { state: "idle" } })}
        providerName="Fixture provider"
        model="Fixture model"
        renderIdentity={(identity) => <button aria-label="Choose fixture model" onClick={chooseModel}>{identity}</button>}
        channelLoadingLabel={row === "model" ? label : undefined}
      />,
    ));
  };
  try {
    await render(false);
    const modelRow = host.firstElementChild;
    const picker = host.querySelector<HTMLButtonElement>('button[aria-label="Choose fixture model"]');
    expect(host.childElementCount).toBe(1);
    await render(true);
    // The shared activity dots, not a separate clock: one loader everywhere.
    const icon = host.querySelector(`[role="status"][aria-label="${label}"]`);
    expect(icon?.querySelector(".activity-dots")).not.toBeNull();
    expect(modelRow?.contains(icon)).toBe(true);
    expect(host.querySelector('button[aria-label="Choose fixture model"]')).toBe(picker);
    await act(async () => picker?.click());
    expect(chooseModel).toHaveBeenCalledTimes(1);
    expect(host.firstElementChild).toBe(modelRow);
    expect(host.childElementCount).toBe(1);
    expect(host.querySelector('button[aria-label="Choose fixture model"]')).toBe(picker);
    expect(host.querySelectorAll("div")).toHaveLength(1);
    await render(false);
    expect(host.querySelector(`[aria-label="${label}"]`)).toBeNull();
    expect(host.firstElementChild).toBe(modelRow);
    expect(host.childElementCount).toBe(1);
  } finally {
    await act(async () => root.unmount());
  }
});

it("renders runtime elapsed and completed duration in the existing indicator only", async () => {
  const clock = vi.spyOn(Date, "now").mockReturnValue(165_000);
  const host = document.createElement("div");
  const root = createRoot(host);
  const runtime = { state: "loaded" as const, epoch: 2, active_turn_started_at_ms: 100_000, last_turn_duration_ms: 12_000 };
  try {
    for (const state of ["working", "idle"] as const) {
      const activity = tabActivity({ backing: { kind: "chat_backend", plugin_id: "fixture", session_id: "thread" }, agent_state: { state } });
      const projected = runtimeActivityLabel(activity.label, activity.kind, runtime, 165_000);
      await act(async () => root.render(<AgentStateIndicator activity={activity} runtimeState={runtime} />));
      expect(host.childElementCount).toBe(1);
      expect(host.querySelectorAll('[role="status"]')).toHaveLength(1);
      expect(host.querySelector('[role="status"]')?.getAttribute("aria-label")).toBe(`Agent ${projected}`);
      expect(host.textContent).toContain(projected);
    }
  } finally {
    await act(async () => root.unmount());
    clock.mockRestore();
  }
});

it("clears the working indicator on provider completion despite later background output", async () => {
  const host = document.createElement("div");
  const root = createRoot(host);
  const ctx: RuntimeContext = {epoch:1,bindingNonce:"binding",tabId:"tab",threadId:"thread",cwd:"/fixture",hostVersion:"1",settings:{},providerSettings:{},claimedSends:[],activeTurns:[],threadParents:[]};
  let state: JsonValue = null;
  let live: "working" | "idle" = "idle";
  const note = (method:string,params:Record<string,JsonValue>):RuntimeFrame => ({kind:"notification",method,params:{threadId:"thread",...params}});
  const feed = async (frame:RuntimeFrame) => {
    const result = runtimeReduce(ctx,state,[frame]);
    state = result.state;
    for (const event of result.events) if (event.kind === "turnState" && event.state !== "needs_input") live = event.state;
    const activity = tabActivity({backing:{kind:"chat_backend",plugin_id:"fixture",session_id:"thread"},agent_state:{state:live}},"Working");
    await act(async()=>root.render(<AgentStateIndicator activity={activity} onStop={()=>{}} activeTool={{toolName:"command",toolInput:{command:"background"}}} />));
    return activity;
  };
  try {
    expect((await feed(note("turn/started",{turn:{id:"turn"}}))).active).toBe(true);
    expect(host.querySelector('button[aria-label="Stop the agent"]:not([aria-hidden="true"])')).not.toBeNull();
    expect((await feed(note("turn/completed",{turn:{id:"turn",status:"completed"}}))).active).toBe(false);
    expect((await feed(note("item/commandExecution/outputDelta",{itemId:"background",delta:"output"}))).status).toBe("Idle");
    expect(host.querySelector('[role="status"]')?.getAttribute("aria-label")).toBe("Agent idle");
    expect(host.querySelector('button[aria-label="Stop the agent"]:not([aria-hidden="true"])')).toBeNull();
  } finally {
    await act(async()=>root.unmount());
  }
});
