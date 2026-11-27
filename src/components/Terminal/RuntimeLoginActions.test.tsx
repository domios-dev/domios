import {act, useState} from "react";
import {createRoot} from "react-dom/client";
import {expect,it,vi} from "vitest";
import { AgentRuntimeBanner } from "./AgentRuntimeBanner";
import {RuntimeLoginActions} from "./RuntimeLoginActions";
import {mergeRemoteRuntimeLoginReply} from "../../lib/cloudBootstrap/model";
import {runtimeStateFromSnapshot} from "../../lib/agentRuntimeState";
import type {AgentRuntimeState} from "../../lib/agentRuntimeState";
const bridge=vi.hoisted(()=>({invoke:vi.fn(),openUrl:vi.fn()}));
vi.mock("../../lib/bridge",()=>({invoke:bridge.invoke}));
vi.mock("../../lib/nativeBridge",()=>({nativeBridge:{openUrl:bridge.openUrl}}));
vi.mock("../../store/multiplexer/hooks",()=>({useMuxSelector:(select:(state:unknown)=>unknown)=>select({tabs:{owned:{session_binding_nonce:"own-nonce",agent_session_id:"own-session"}}})}));
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT=true;

it.each([
  { state: "loaded", epoch: 1, auth: "signedIn", loginMethods: ["browser"] },
  { state: "loaded", epoch: 1, authentication: { state: "authenticated", methods: [] } },
] satisfies AgentRuntimeState[])("renders no sign-in heading or controls for authenticated runtime state", async state => {
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<RuntimeLoginActions tabId="owned" state={state} />));
    expect(host.querySelector('h3, button, [role="group"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
  }
});

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
    expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("agent_runtime_login",{tabId:"mesh:opaque-peer/owned",request:{action:"notify",loginId:"opaque-id"}});
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
      tabId: "own", request: { action: "code", code: "opaque-code", providerId: "fixture", loginId: "own" },
    });
  } finally { await act(async () => root.unmount()); }
});

it("renders declared authentication methods and submits only an available method ID", async () => {
  bridge.invoke.mockReset();bridge.openUrl.mockReset();
  bridge.invoke.mockResolvedValue({});
  const host=document.createElement("div");const root=createRoot(host);
  const state:AgentRuntimeState={state:"loaded",epoch:4,authentication:{state:"unknown",methods:[
    {id:"opaque-agent",name:"Agent sign-in",available:true},
    {id:"opaque-terminal",name:"Terminal sign-in",available:false,unavailableReason:"Unsupported extra challenge"},
  ]}};
  try {
    await act(async()=>root.render(<RuntimeLoginActions tabId="owned" state={state}/>));
    const group=host.querySelector('[role="group"][aria-label="Sign in to the agent"]');
    expect(group).not.toBeNull();
    // No raw state word in the card.
    expect(host.querySelector('output[aria-label="Authentication state"]')).toBeNull();
    const buttons=Array.from(group!.querySelectorAll("button"));
    const terminal=buttons.find(button=>button.textContent === "Terminal sign-in")!;
    expect(terminal.disabled).toBe(true);
    expect(terminal.getAttribute("aria-description")).toBe("Unsupported extra challenge");
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
  const state:AgentRuntimeState={state:"loginPending",epoch:2,loginBinding:{nonce:"own-nonce",epoch:2},authentication:{state:"needs_auth",methods:[
    {id:"opaque-key",name:"Key sign-in",available:true,input:"apiKey"},
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
    expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("agent_runtime_login",{tabId:"owned",request:{methodId:"opaque-key",apiKey:"opaque-secret",binding:{nonce:"own-nonce",epoch:2}}});
    expect(host.querySelector('input[aria-label="API key"]')).toBeNull();
  } finally {await act(async()=>root.unmount());}
});

it("keeps a non-actionable runtime diagnostic out of chat", async () => {
  const host=document.createElement("div");document.body.append(host);const root=createRoot(host);
  const state:AgentRuntimeState={state:"loaded",epoch:4,notice:"Agent notice"};
  try {
    await act(async()=>root.render(<AgentRuntimeBanner tabId="owned" state={state} onRetry={() => {}} />));
    expect(host.querySelector('[role="status"]')).toBeNull();
    expect(host.textContent).toBe("");
  } finally {await act(async()=>root.unmount());host.remove();}
});

it("drops old sign-in results and secret input when the runtime binding changes", async () => {
  bridge.invoke.mockReset();bridge.openUrl.mockReset();
  let finish!:(value:unknown)=>void;
  bridge.invoke.mockImplementationOnce(()=>new Promise(resolve=>{finish=resolve;})).mockResolvedValue({});
  const host=document.createElement("div");const root=createRoot(host);
  const state:AgentRuntimeState={state:"loginPending",epoch:1,loginBinding:{nonce:"old-nonce",epoch:1},authentication:{state:"needs_auth",methods:[
    {id:"opaque-oauth",name:"Browser method",available:true,input:"none"},
    {id:"opaque-key",name:"Key method",available:true,input:"apiKey"},
  ]}};
  try {
    await act(async()=>root.render(<RuntimeLoginActions tabId="owned" state={state}/>));
    await act(async()=>Array.from(host.querySelectorAll("button")).find(button=>button.textContent==="Browser method")!.click());
    const fresh={...state,epoch:2,loginBinding:{nonce:"new-nonce",epoch:2}};
    await act(async()=>root.render(<RuntimeLoginActions tabId="owned" state={fresh}/>));
    await act(async()=>finish({login:{status:"pending",url:"https://example.test/old-account"}}));
    expect(bridge.openUrl).not.toHaveBeenCalled();
    const key=Array.from(host.querySelectorAll("button")).find(button=>button.textContent==="Key method")!;
    expect(key.disabled).toBe(false);
    await act(async()=>key.click());
    const input=host.querySelector<HTMLInputElement>('input[aria-label="API key"]')!;
    await act(async()=>{
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value")!.set!.call(input,"ephemeral-key");
      input.dispatchEvent(new Event("input",{bubbles:true}));
    });
    await act(async()=>root.render(<RuntimeLoginActions tabId="different" state={fresh}/>));
    expect(host.querySelector('input[aria-label="API key"]')).toBeNull();
    expect(host.textContent).not.toContain("ephemeral-key");
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    expect(bridge.invoke).toHaveBeenCalledWith("agent_runtime_login",{tabId:"owned",request:{methodId:"opaque-oauth",binding:{nonce:"old-nonce",epoch:1}}});
  } finally {await act(async()=>root.unmount());}
});

it("submits a challenge code once and allows cancellation while its callback is pending", async () => {
  bridge.invoke.mockReset();bridge.openUrl.mockReset();
  let fail!:(reason:unknown)=>void;
  bridge.invoke.mockImplementationOnce(()=>new Promise((_resolve,reject)=>{fail=reject;})).mockResolvedValue({});
  const host=document.createElement("div");const root=createRoot(host);
  const state:AgentRuntimeState={state:"loginPending",epoch:3,loginBinding:{nonce:"own-nonce",epoch:3},
    login:{status:"pending",loginId:"host-login",url:"https://example.test/login",codeInput:true,instructions:"Challenge instructions"}};
  try {
    await act(async()=>root.render(<RuntimeLoginActions tabId="owned" state={state}/>));
    expect(host.textContent).toContain("Challenge instructions");
    const input=host.querySelector<HTMLInputElement>('input[aria-label="Sign-in code"]')!;
    expect(input.type).toBe("password");
    await act(async()=>{
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value")!.set!.call(input,"one-time-secret");
      input.dispatchEvent(new Event("input",{bubbles:true}));
    });
    const form=host.querySelector('form[aria-label="Sign-in code"]')!;
    await act(async()=>{form.dispatchEvent(new Event("submit",{bubbles:true,cancelable:true}));form.dispatchEvent(new Event("submit",{bubbles:true,cancelable:true}));});
    expect(input.value).toBe("");
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    await act(async()=>Array.from(host.querySelectorAll("button")).find(button=>button.textContent==="Cancel")!.click());
    expect(bridge.invoke).toHaveBeenNthCalledWith(1,"agent_runtime_login",{tabId:"owned",request:{action:"code",code:"one-time-secret",loginId:"host-login",binding:{nonce:"own-nonce",epoch:3}}});
    expect(bridge.invoke).toHaveBeenNthCalledWith(2,"agent_runtime_login",{tabId:"owned",request:{action:"cancel",loginId:"host-login",binding:{nonce:"own-nonce",epoch:3}}});
    await act(async()=>root.render(<RuntimeLoginActions tabId="owned" state={{...state,login:{status:"canceled",loginId:"host-login"}}}/>));
    await act(async()=>fail("stale one-time-secret failure"));
    expect(host.textContent).not.toContain("stale");
    expect(host.textContent).not.toContain("one-time-secret");
    expect(bridge.openUrl).not.toHaveBeenCalled();
  } finally {await act(async()=>root.unmount());}
});

it("keeps a remote key method disabled with an accessible explanation", async () => {
  bridge.invoke.mockReset();bridge.invoke.mockResolvedValue({});
  const host=document.createElement("div");const root=createRoot(host);
  try {
    await act(async()=>root.render(<RuntimeLoginActions tabId="mesh:peer/owned" state={{state:"loginPending",epoch:1,authentication:{state:"needs_auth",methods:[
      {id:"opaque-key",name:"Key method",available:true,input:"apiKey"},
    ]}}}/>));
    const button=Array.from(host.querySelectorAll("button")).find(button=>button.textContent==="Key method")!;
    expect(button.disabled).toBe(true);
    expect(button.getAttribute("aria-description")).toBeTruthy();
    await act(async()=>button.click());
    expect(bridge.invoke).not.toHaveBeenCalled();
  } finally {await act(async()=>root.unmount());}
});

it("keeps a live notice across same-epoch snapshots and clears it for a new runtime", () => {
  const state:AgentRuntimeState={state:"loaded",epoch:4,notice:"Transient"};
  expect(runtimeStateFromSnapshot(state,{state:"loaded",epoch:4})?.notice).toBe("Transient");
  expect(runtimeStateFromSnapshot(state,{state:"loaded",epoch:5})?.notice).toBeUndefined();
});

it("restarts the remembered browser choice once after an explicit different-account retry",async()=>{
  bridge.invoke.mockReset().mockResolvedValue({});bridge.openUrl.mockReset();
  const host=document.createElement("div");const root=createRoot(host);
  const state:AgentRuntimeState={state:"loginPending",epoch:3,auth:"signedOut",loginMethods:["browser"],loginChoices:[{id:"work-browser",label:"Browser"}],preferredLoginMethod:{method:"browser",choice:"work-browser",apiKey:false}};
  try {
    await act(async()=>root.render(<RuntimeLoginActions tabId="private-retry" state={state}/>));
    await act(async()=>root.render(<RuntimeLoginActions tabId="private-retry" state={{...state}}/>));
    expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("agent_runtime_login",{tabId:"private-retry",request:{method:"browser",account:"work-browser"}});
  }finally{await act(async()=>root.unmount());}
});
it("remembers only the API-key method and asks for a fresh masked key on retry",async()=>{
  bridge.invoke.mockReset();
  const host=document.createElement("div");const root=createRoot(host);
  const state:AgentRuntimeState={state:"loginPending",epoch:3,auth:"signedOut",loginMethods:["apiKey"],preferredLoginMethod:{method:"apiKey",apiKey:true}};
  try {
    await act(async()=>root.render(<RuntimeLoginActions tabId="private-retry-key" state={state}/>));
    const input=host.querySelector<HTMLInputElement>('input[aria-label="API key"]');
    expect(input?.type).toBe("password");expect(input?.value).toBe("");
    expect(bridge.invoke).not.toHaveBeenCalled();
  }finally{await act(async()=>root.unmount());}
});

it("renders remote sign-in card, opens link in owner browser, and submits masked code once with exact flow binding", async () => {
  bridge.invoke.mockReset();
  bridge.openUrl.mockReset();
  bridge.openUrl.mockResolvedValue(undefined);
  let resolveSubmit!: (value: unknown) => void;
  bridge.invoke.mockImplementationOnce(() => new Promise(resolve => { resolveSubmit = resolve; }));
  const remoteTabId = "mesh:4974b28eec1a4248bc44693b2d8c4afb/004c7c5c";
  const host = document.createElement("div");
  const root = createRoot(host);
  const state: AgentRuntimeState = {
    state: "loginPending",
    epoch: 4,
    auth: "signedOut",
    loginProviderName: "Claude Code",
    loginCodeInput: true,
    loginChoices: [
      { id: "subscription", label: "Subscription" },
      { id: "console", label: "Console" },
      { id: "sso", label: "SSO" },
    ],
    loginBinding: {
      installationId: "4974b28eec1a4248bc44693b2d8c4afb",
      tabId: "004c7c5c",
      providerId: "claude",
      sessionId: "sess-remote",
      nonce: "remote-nonce",
      epoch: 4,
    },
    login: {
      status: "pending",
      loginId: "remote-flow-9",
      method: "browser",
      url: "https://claude.ai/oauth/authorize?state=remote-flow-9",
    },
  };
  try {
    await act(async () => root.render(<RuntimeLoginActions tabId={remoteTabId} providerId="claude" state={state} />));
    expect(host.querySelector('output[aria-label="Sign-in status"]')?.getAttribute("data-login-status")).toBe("pending");
    const link = host.querySelector<HTMLAnchorElement>('a[aria-label="Open sign-in page"]')!;
    await act(async () => { link.click(); });
    expect(bridge.openUrl).toHaveBeenCalledExactlyOnceWith("https://claude.ai/oauth/authorize?state=remote-flow-9");

    const input = host.querySelector<HTMLInputElement>('input[aria-label="Sign-in code"]')!;
    expect(input.type).toBe("password");
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "remote-pkce-secret");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const form = host.querySelector('form[aria-label="Sign-in code"]')!;
    await act(async () => {
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
      form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    expect(input.value).toBe("");
    expect(host.querySelector('output[aria-label="Sign-in status"]')?.getAttribute("data-login-status")).toBe("submitted");
    expect(bridge.invoke).toHaveBeenCalledExactlyOnceWith("agent_runtime_login", {
      tabId: remoteTabId,
      request: {
        action: "code",
        code: "remote-pkce-secret",
        providerId: "claude",
        loginId: "remote-flow-9",
        binding: {
          installationId: "4974b28eec1a4248bc44693b2d8c4afb",
          tabId: "004c7c5c",
          providerId: "claude",
          sessionId: "sess-remote",
          nonce: "remote-nonce",
          epoch: 4,
        },
      },
    });
    await act(async () => resolveSubmit({ outcome: "accepted", login: state.login }));
    expect(host.querySelector('output[aria-label="Sign-in status"]')?.getAttribute("data-login-status")).toBe("accepted");
    expect(host.querySelector('form[aria-label="Sign-in code"]')).toBeNull();
    expect(host.textContent).not.toContain("remote-pkce-secret");
  } finally {
    await act(async () => root.unmount());
  }
});

it("shows uncertain status on remote code submission uncertainty and prevents replay", async () => {
  bridge.invoke.mockReset();
  bridge.invoke.mockResolvedValue({ outcome: "uncertain", reason: "Flush unconfirmed" });
  const remoteTabId = "mesh:4974b28eec1a4248bc44693b2d8c4afb/004c7c5c";
  const host = document.createElement("div");
  const root = createRoot(host);
  const state: AgentRuntimeState = {
    state: "loginPending",
    epoch: 5,
    auth: "signedOut",
    loginCodeInput: true,
    loginBinding: {
      installationId: "4974b28eec1a4248bc44693b2d8c4afb",
      tabId: "004c7c5c",
      providerId: "claude",
      nonce: "remote-nonce",
      epoch: 5,
    },
    login: {
      status: "pending",
      loginId: "remote-flow-10",
      method: "browser",
      url: "https://claude.ai/oauth/authorize?state=remote-flow-10",
    },
  };
  try {
    await act(async () => root.render(<RuntimeLoginActions tabId={remoteTabId} providerId="claude" state={state} />));
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Sign-in code"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "secret-uncertain-code");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => host.querySelector('form[aria-label="Sign-in code"]')!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
    const status = host.querySelector('output[aria-label="Sign-in status"]');
    expect(status?.getAttribute("data-login-status")).toBe("uncertain");
    expect(host.querySelector('form[aria-label="Sign-in code"]')).toBeNull();
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    expect(host.textContent).not.toContain("secret-uncertain-code");
  } finally {
    await act(async () => root.unmount());
  }
});

it("preserves uncertain and refused code reasons when a remote card merges updated loginBinding and epoch", async () => {
  const remoteTabId = "mesh:opaque-peer/tab-remote-code";
  function StatefulRemoteCard({ initial }: { initial: AgentRuntimeState }) {
    const [state, setState] = useState(initial);
    return (
      <RuntimeLoginActions
        tabId={remoteTabId}
        providerId="opaque-provider"
        state={state}
        onLoginReply={(reply) => setState((prev) => mergeRemoteRuntimeLoginReply(prev, reply))}
      />
    );
  }
  const baseState: AgentRuntimeState = {
    state: "loginPending",
    epoch: 1,
    auth: "signedOut",
    loginCodeInput: true,
    loginProviderName: "Opaque Provider",
    loginBinding: {
      installationId: "opaque-peer",
      tabId: "tab-remote-code",
      providerId: "opaque-provider",
      nonce: "nonce-1",
      epoch: 1,
    },
    login: {
      status: "pending",
      loginId: "opaque-flow-20",
      method: "browser",
      url: "https://example.invalid/opaque-login",
    },
  };

  // 1. Uncertain outcome with changed nonce/epoch keeps uncertain status + reason and locks replay.
  bridge.invoke.mockReset();
  bridge.invoke.mockResolvedValue({
    outcome: "uncertain",
    reason: "Flush unconfirmed",
    login: baseState.login,
    loginBinding: { ...baseState.loginBinding, nonce: "nonce-2", epoch: 2 },
  });
  let host = document.createElement("div");
  let root = createRoot(host);
  try {
    await act(async () => root.render(<StatefulRemoteCard initial={baseState} />));
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Sign-in code"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "opaque-secret-1");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () =>
      host.querySelector('form[aria-label="Sign-in code"]')!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    const status = host.querySelector('output[aria-label="Sign-in status"]');
    expect(status?.getAttribute("data-login-status")).toBe("uncertain");
    expect(status?.textContent).toContain("Flush unconfirmed");
    expect(host.querySelector('form[aria-label="Sign-in code"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
  }

  // 2. Refused outcome with changed nonce/epoch keeps refused reason visible.
  bridge.invoke.mockReset();
  bridge.invoke.mockResolvedValue({
    outcome: "refused",
    reason: "Invalid verification code",
    login: baseState.login,
    loginBinding: { ...baseState.loginBinding, nonce: "nonce-3", epoch: 3 },
  });
  host = document.createElement("div");
  root = createRoot(host);
  try {
    await act(async () => root.render(<StatefulRemoteCard initial={baseState} />));
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Sign-in code"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "opaque-secret-2");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () =>
      host.querySelector('form[aria-label="Sign-in code"]')!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
    );
    const status = host.querySelector('output[aria-label="Sign-in status"]');
    expect(status?.getAttribute("data-login-status")).toBe("pending");
    expect(host.textContent).toContain("Invalid verification code");
    expect(host.querySelector('form[aria-label="Sign-in code"]')).not.toBeNull();
  } finally {
    await act(async () => root.unmount());
  }
});
