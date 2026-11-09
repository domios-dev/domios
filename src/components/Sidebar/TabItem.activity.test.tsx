import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { __setInvokeForTest } from "../../lib/bridge";
import { AgentStateIndicator } from "../Terminal/AgentStateIndicator";
import { TabRecoveryAction } from "../Terminal/TabRecoveryAction";
import { StatusLineModelPicker } from "../Terminal/StatusLineModelPicker";
import { runtimeStatusLineIdentity } from "../Terminal/statusLinePickerModel";
import { tabActivity } from "../../lib/tabActivity";
import type { ProviderDescriptor, Tab } from "../../lib/types";
import { TabItem } from "./TabItem";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it.each(["Polish the sidebar", undefined])("temporarily shows compaction in the description slot (description: %s)", async normalDescription => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const tab: Tab = {
    id: "swift", title: "Swift", tab_type: "Terminal", pty_id: null, cwd: null,
    agent_type: "fixture-agent", agent_session_id: "session", description: normalDescription,
  };
  try {
    for (const phase of ["work", "compacting", "work"] as const) {
      await act(async () => root.render(<TabItem
        tab={{ ...tab, agent_state: { state: "working", phase } }} hasAgent status="Working"
        index={0} isActive={false} onClick={() => undefined} onClose={() => undefined}
        canClose workspaceId="fixture" />));
      const activation = host.querySelector<HTMLButtonElement>('button[aria-label="Open tab Swift"]');
      expect(activation).not.toBeNull();
      expect(activation?.querySelector('[data-testid="tab-title"]')?.parentElement?.textContent).toBe("Swift");
      const description = activation?.querySelector('[data-testid="tab-description-subtitle"]');
      expect(description?.textContent).toBe(phase === "compacting" ? "Compacting context…" : normalDescription);
      if (phase === "compacting") {
        expect(activation?.textContent).not.toContain("Polish the sidebar");
        expect(activation?.querySelector('[role="img"][aria-label="Compacting context…"]')).not.toBeNull();
        expect(host.querySelector('[role="status"]')?.textContent).toContain("Compacting context…");
      } else {
        expect(activation?.textContent).not.toContain("Compacting context…");
      }
    }
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("projects runtime launch failure and recovery into sidebar and status-line accessible names", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const base: Tab = {
    id: "runtime", title: "Runtime", tab_type: "Terminal", pty_id: null, cwd: null,
    agent_type: "fixture-runtime", backing: {kind:"chat_backend",plugin_id:"fixture-runtime",session_id:""},
  };
  try {
    for (const reason of ["Provider refused: protocol_error", "Provider operation failed: session/new rejected"]) {
      const tab: Tab = {...base, agent_runtime_state:{state:"degraded",epoch:1,reason}};
      await act(async () => root.render(<>
        <TabItem tab={tab} hasAgent status="Idle" index={0} isActive={false}
          onClick={() => undefined} onClose={() => undefined} canClose workspaceId="fixture" />
        <AgentStateIndicator tab={tab} activity={tabActivity(tab,"Idle")} providerName="Fixture" />
      </>));
      expect(host.querySelector('[role="status"][aria-label="Runtime tab status Error"]')).not.toBeNull();
      expect(host.querySelector('[role="status"][aria-label="Agent Fixture · Launch failed"]')).not.toBeNull();
      expect(host.textContent).not.toContain("Starting…");
    }
    const recovered: Tab = {...base, backing:{kind:"chat_backend",plugin_id:"fixture-runtime",session_id:"own"},
      agent_session_id:"own",agent_runtime_state:{state:"loaded",epoch:2}};
    await act(async () => root.render(<TabItem tab={recovered} hasAgent status="Idle" index={0} isActive={false}
      onClick={() => undefined} onClose={() => undefined} canClose workspaceId="fixture" />));
    expect(host.querySelector('[role="status"][aria-label="Runtime tab status Idle"]')).not.toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("names a degraded live session truthfully and offers its restart, which targets this tab once", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const requests: Array<[string, unknown]> = [];
  __setInvokeForTest(async <T,>(command: string, args?: unknown) => { requests.push([command, args]); return {} as T; });
  const tab: Tab = {
    id: "live-runtime", title: "Live", tab_type: "Terminal", pty_id: null, cwd: null, agent_type: "fixture-runtime",
    agent_session_id: "own", backing: {kind:"chat_backend",plugin_id:"fixture-runtime",session_id:"own"}, boot_stage: "agent_ready",
    agent_runtime_state: {state:"degraded",epoch:1,failureCode:"provider_refused",reason:"opaque deadline exceeded; runtime recovery required"},
  };
  try {
    await act(async () => root.render(<AgentStateIndicator tab={tab} activity={tabActivity(tab,"Working")} providerName="Fixture"
      recovery={<TabRecoveryAction tabId={tab.id} onError={() => undefined} />} />));
    expect(host.querySelector('[role="status"][aria-label="Agent Fixture · Runtime needs recovery"]')).not.toBeNull();
    const restart = host.querySelector<HTMLButtonElement>('button[aria-label="Restart the agent"]');
    expect(restart).not.toBeNull();
    await act(async () => { restart!.click(); });
    expect(requests).toEqual([["restart_agent_tab", {tabId:"live-runtime"}]]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setInvokeForTest(null);
  }
});

it.each([false,true,undefined])("names a runtime agent by its declared model, never its own name and version (model: %s)", async hasModel => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const controls = hasModel ? {configOptions:[{id:"engine-choice",name:"Engine",category:"model",currentValue:"wire-one",options:[{value:"wire-one",name:"First declared"}]}]} : undefined;
  const runtime = {state:"degraded" as const,epoch:1,reason:"Provider refused: protocol_error",agentName:hasModel === undefined ? undefined : "Wire Agent",binaryVersion:hasModel === undefined ? undefined : "0.0.1",controls};
  const tab: Tab = {id:"runtime",title:"Runtime",tab_type:"Terminal",pty_id:null,cwd:null,
    backing:{kind:"chat_backend",plugin_id:"fixture-runtime",session_id:""},agent_runtime_state:runtime};
  const descriptor = {id:"fixture-runtime",display_name:"Harness",agent_runtime:{},available_models:[],model_switch:"live"} as unknown as ProviderDescriptor;
  // The agent's own name and version stay out of the label; the declared model reads like any provider's.
  const identity = hasModel ? "First declared" : "Harness";
  try {
    await act(async () => root.render(<AgentStateIndicator tab={tab} activity={tabActivity(tab,"Idle")}
      runtimeState={runtime} providerName="Harness" model="Unknown model"
      identityLabel={runtimeStatusLineIdentity(tab.agent_runtime_state) || descriptor.display_name}
      renderIdentity={label => <StatusLineModelPicker tabId={tab.id} descriptor={descriptor}
        runtimeControls={controls} runtimeIdentity={runtime} facts={{hasSession:true}}
        label={label} modelCandidates={["Unknown model"]} currentEffort={undefined} />} />));
    const status = host.querySelector(`[role="status"][aria-label="Agent ${identity} · Launch failed"]`);
    expect(status).not.toBeNull();
    expect(status?.textContent).toContain(identity);
    expect(status?.textContent).not.toContain("Unknown model");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
