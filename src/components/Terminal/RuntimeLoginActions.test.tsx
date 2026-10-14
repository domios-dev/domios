import {act} from "react";
import {createRoot} from "react-dom/client";
import {expect,it,vi} from "vitest";
import {RuntimeLoginActions} from "./RuntimeLoginActions";
import type {AgentRuntimeState} from "../../lib/agentRuntimeState";
const bridge=vi.hoisted(()=>({invoke:vi.fn(),openUrl:vi.fn()}));
vi.mock("../../lib/bridge",()=>({invoke:bridge.invoke}));
vi.mock("../../lib/nativeBridge",()=>({nativeBridge:{openUrl:bridge.openUrl}}));
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT=true;

it("starts browser login once, opens the native browser, and renders the device result",async()=>{
  const host=document.createElement("div");const root=createRoot(host);
  const state:AgentRuntimeState={state:"degraded",epoch:1,auth:"signedOut",loginMethods:["browser","device","apiKey"],loginProviderName:"Fixture"};
  let resolve!:(value:unknown)=>void;
  bridge.invoke.mockImplementation(()=>new Promise(done=>{resolve=done;}));
  bridge.openUrl.mockResolvedValue(undefined);
  try {
    await act(async()=>root.render(<RuntimeLoginActions tabId="owned" state={state}/>));
    const button=Array.from(host.querySelectorAll("button")).find(button=>button.textContent === "Continue in browser")!;
    await act(async()=>{button.click();button.click();});
    expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("agent_runtime_login",{tabId:"owned",request:{method:"browser"}});
    await act(async()=>resolve({login:{status:"pending",url:"https://example.test/login"}}));
    expect(bridge.openUrl).toHaveBeenCalledExactlyOnceWith("https://example.test/login");
    await act(async()=>root.render(<RuntimeLoginActions tabId="owned" state={{...state,login:{status:"pending",loginId:"fixture",url:"https://example.test/device",userCode:"ABCD-1234"}}}/>));
    expect(host.querySelector('output[aria-label="Device code"]')?.textContent).toBe("ABCD-1234");
    expect(host.querySelector('a[aria-label="Open sign-in page"]')?.getAttribute("href")).toBe("https://example.test/device");
    expect(host.querySelector('button[aria-label="Copy device code"]')).not.toBeNull();
  } finally {await act(async()=>root.unmount());}
});
