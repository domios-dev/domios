import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { useAgentRuntimeState } from "./useAgentRuntimeState";
import type { AgentRuntimeState } from "../../lib/agentRuntimeState";
const bridge=vi.hoisted(()=>({invoke:vi.fn(),listen:vi.fn()}));
vi.mock("../../lib/bridge",()=>bridge);
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT=true;

it("routes loaded-history Retry to the checkpoint endpoint and accepts the resulting history state",async()=>{
  bridge.listen.mockResolvedValue(()=>{});
  bridge.invoke.mockResolvedValue({state:"loaded",epoch:1,history:"complete",historyPages:12});
  const host=document.createElement("div"),root=createRoot(host);
  const snapshot:AgentRuntimeState={state:"loaded",epoch:1,history:"failed",historyPages:8,historyError:"fixture-deadline"};
  let view:ReturnType<typeof useAgentRuntimeState>|undefined;
  function Harness(){view=useAgentRuntimeState("own-tab","nonce",snapshot);return null;}
  try {
    await act(async()=>root.render(<Harness/>));
    await act(async()=>view!.retry());
    expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("agent_runtime_retry",{tabId:"own-tab"});
    expect(view!.state).toMatchObject({state:"loaded",history:"complete",historyPages:12});
  } finally {await act(async()=>root.unmount());}
});
