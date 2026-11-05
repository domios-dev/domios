import {act} from "react";
import {createRoot} from "react-dom/client";
import {expect,it,vi} from "vitest";
import {RuntimeDeclaredControls} from "./RuntimeDeclaredControls";
import {RuntimeLoginActions} from "./RuntimeLoginActions";
import {runtimeStateFromSnapshot} from "../../lib/agentRuntimeState";
import type {AgentRuntimeState} from "../../lib/agentRuntimeState";
const bridge=vi.hoisted(()=>({invoke:vi.fn(),openUrl:vi.fn()}));
vi.mock("../../lib/bridge",()=>({invoke:bridge.invoke}));
vi.mock("../../lib/nativeBridge",()=>({nativeBridge:{openUrl:bridge.openUrl}}));
vi.mock("../../store/multiplexer/hooks",()=>({useMuxSelector:(select:(state:unknown)=>unknown)=>select({tabs:{owned:{session_binding_nonce:"own-nonce",agent_session_id:"own-session"}}})}));
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

it("renders declared authentication methods and submits only an available method ID", async () => {
  bridge.invoke.mockReset();bridge.openUrl.mockReset();
  bridge.invoke.mockResolvedValue({});
  const host=document.createElement("div");const root=createRoot(host);
  const state:AgentRuntimeState={state:"loaded",epoch:4,authentication:{state:"unknown",methods:[
    {id:"opaque-agent",name:"Agent sign-in",available:true},
    {id:"opaque-terminal",name:"Terminal sign-in",available:false},
  ]}};
  try {
    await act(async()=>root.render(<RuntimeLoginActions tabId="owned" state={state}/>));
    const group=host.querySelector('[role="group"][aria-label="Sign in to the provider"]');
    expect(group).not.toBeNull();
    expect(host.querySelector('output[aria-label="Authentication state"]')?.textContent).toBe("unknown");
    const buttons=Array.from(group!.querySelectorAll("button"));
    const terminal=buttons.find(button=>button.textContent === "Terminal sign-in")!;
    expect(terminal.disabled).toBe(true);
    await act(async()=>terminal.click());
    expect(bridge.invoke).not.toHaveBeenCalled();
    const agent=buttons.find(button=>button.textContent === "Agent sign-in")!;
    await act(async()=>agent.click());
    expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("agent_runtime_login",{tabId:"owned",request:{methodId:"opaque-agent"}});
    expect(bridge.openUrl).not.toHaveBeenCalled();
  } finally {await act(async()=>root.unmount());}
});

it("asks for a key before a key-taking declared method and sends both together", async () => {
  bridge.invoke.mockReset();bridge.invoke.mockResolvedValue({});
  const host=document.createElement("div");const root=createRoot(host);
  const state:AgentRuntimeState={state:"loginPending",epoch:2,apiKeyMethods:["opaque-key"],authentication:{state:"needs_auth",methods:[
    {id:"opaque-key",name:"Key sign-in",available:true},
  ]}};
  try {
    await act(async()=>root.render(<RuntimeLoginActions tabId="owned" state={state}/>));
    const method=Array.from(host.querySelectorAll("button")).find(button=>button.textContent === "Key sign-in")!;
    await act(async()=>method.click());
    expect(bridge.invoke).not.toHaveBeenCalled();
    const input=host.querySelector<HTMLInputElement>('input[aria-label="API key"]')!;
    expect(input.type).toBe("password");
    await act(async()=>{
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value")!.set!.call(input,"opaque-secret");
      input.dispatchEvent(new Event("input",{bubbles:true}));
    });
    await act(async()=>host.querySelector('form[aria-label="API key sign-in"]')!.dispatchEvent(new Event("submit",{bubbles:true,cancelable:true})));
    expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("agent_runtime_login",{tabId:"owned",request:{methodId:"opaque-key",apiKey:"opaque-secret"}});
    expect(host.querySelector('input[aria-label="API key"]')).toBeNull();
  } finally {await act(async()=>root.unmount());}
});

it("renders a session config picker, sends its pinned selection, and dismisses transient notices", async () => {
  bridge.invoke.mockReset();bridge.invoke.mockResolvedValue({ok:true});
  const host=document.createElement("div");const root=createRoot(host);
  const state:AgentRuntimeState={state:"loaded",epoch:4,notice:"Agent notice",controls:{configOptions:[
    {id:"policy",name:"Policy",currentValue:"safe",options:[{value:"safe",name:"Safe"},{value:"review",name:"Review"}]},
    {id:"readonly",name:"Reported value",currentValue:"opaque",options:[]},
  ]}};
  try {
    await act(async()=>root.render(<RuntimeDeclaredControls tabId="owned" state={state}/>));
    const picker=host.querySelector<HTMLSelectElement>('select[aria-label="Policy"]')!;
    expect(picker.value).toBe("safe");
    expect(Array.from(picker.options).map(option=>option.textContent)).toEqual(["Safe","Review"]);
    await act(async()=>{picker.value="review";picker.dispatchEvent(new Event("change",{bubbles:true}));});
    expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("agent_runtime_controls",{tabId:"owned",request:{threadId:"own-session",bindingNonce:"own-nonce",epoch:4,id:"policy",value:"review",mode:false}});
    expect(host.querySelector("output")?.textContent).toBe("opaque");
    expect(host.querySelector('[role="status"]')?.textContent).toContain("Agent notice");
    await act(async()=>Array.from(host.querySelectorAll("button")).find(button=>button.textContent === "Dismiss notice")!.click());
    expect(host.querySelector('[role="status"]')).toBeNull();
    bridge.invoke.mockRejectedValue(new Error("Unadvertised value"));
    await act(async()=>{picker.value="safe";picker.dispatchEvent(new Event("change",{bubbles:true}));});
    expect(host.textContent).toContain("The agent refused this control value.");
  } finally {await act(async()=>root.unmount());}
});


it("keeps a live notice across same-epoch snapshots and clears it for a new runtime", () => {
  const state:AgentRuntimeState={state:"loaded",epoch:4,notice:"Transient"};
  expect(runtimeStateFromSnapshot(state,{state:"loaded",epoch:4})?.notice).toBe("Transient");
  expect(runtimeStateFromSnapshot(state,{state:"loaded",epoch:5})?.notice).toBeUndefined();
});

it("keeps declared model, reasoning and session mode controls separately named and focusable", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const state: AgentRuntimeState = {state:"loaded",epoch:4,controls:{
    configOptions:[
      {id:"model",name:"Model",category:"model",currentValue:"model-one",options:[{value:"model-one",name:"Model one"}]},
      {id:"effort",name:"Reasoning effort",currentValue:"high",options:[{value:"high",name:"High"}]},
    ],
    mode:"code",modes:[{id:"code",name:"Code"}],
  }};
  try {
    await act(async () => root.render(<RuntimeDeclaredControls tabId="owned" state={state} />));
    const group = host.querySelector('[role="group"][aria-label="Agent controls"]');
    expect(group).not.toBeNull();
    for (const name of ["Model", "Reasoning effort", "Session mode"]) {
      const control = group!.querySelector<HTMLSelectElement>(`[role="combobox"][aria-label="${name}"]`)!;
      expect(control).not.toBeNull();
      expect(control.disabled).toBe(false);
      expect(control.tabIndex).toBe(0);
      control.focus();
      expect(document.activeElement).toBe(control);
    }
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
