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

it("requests one phone notification for a remote tab without opening a local browser",async()=>{
  bridge.invoke.mockReset(); bridge.openUrl.mockReset();
  bridge.invoke.mockResolvedValue({notificationId:"opaque-notice"});
  const host=document.createElement("div");const root=createRoot(host);
  try {
    await act(async()=>root.render(<RuntimeLoginActions tabId="mesh:opaque-peer/owned" state={{state:"loginPending",epoch:1,login:{status:"pending",loginId:"opaque-id",url:"https://example.invalid/opaque-path",userCode:"opaque-token"}}}/>));
    const button=Array.from(host.querySelectorAll("button")).find(button=>button.textContent === "Send to my phone")!;
    await act(async()=>{button.click();button.click();});
    expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("agent_runtime_login",{tabId:"mesh:opaque-peer/owned",request:{action:"notify"}});
    expect(button.disabled).toBe(true);
    expect(bridge.openUrl).not.toHaveBeenCalled();
  } finally {await act(async()=>root.unmount());}
});

it("submits a provider sign-in code through the masked card and clears it immediately", async () => {
  bridge.invoke.mockReset();
  bridge.invoke.mockResolvedValue({ login: { status: "pending", loginId: "own" } });
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<RuntimeLoginActions tabId="own" providerId="fixture" state={{
      state: "loginPending", epoch: 0, loginCodeInput: true,
      login: { status: "pending", loginId: "own", url: "https://example.test/login" },
    }} />));
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Sign-in code"]')!;
    expect(input.type).toBe("password");
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "opaque-code");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => host.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
    expect(input.value).toBe("");
    expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("agent_runtime_login", {
      tabId: "own", request: { action: "code", code: "opaque-code", providerId: "fixture" },
    });
  } finally { await act(async () => root.unmount()); }
});
