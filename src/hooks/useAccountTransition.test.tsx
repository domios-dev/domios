import {act} from "react";
import {createRoot} from "react-dom/client";
import {expect,it,vi} from "vitest";
const fixture=vi.hoisted(()=>({json:vi.fn(),handlers:new Map<string,(event:{payload:unknown})=>void>()}));
vi.mock("../lib/daemonRuntime",()=>({selectedDaemonTransport:{json:fixture.json}}));
vi.mock("../lib/bridge",()=>({listen:async(name:string,handler:(event:{payload:unknown})=>void)=>{fixture.handlers.set(name,handler);return()=>fixture.handlers.delete(name);}}));
import {useAccountTransition} from "./useAccountTransition";

it("preserves a delivered transition against an earlier GET and retires old tab subscriptions",async()=>{
  Object.assign(globalThis,{IS_REACT_ACT_ENVIRONMENT:true});
  fixture.handlers.clear();
  let resolve!:(value:unknown)=>void;
  fixture.json.mockReset().mockImplementationOnce(()=>new Promise(yes=>{resolve=yes;})).mockResolvedValue({transition:{id:"delivered",providerId:"fixture"},canVerify:false,turnActive:true});
  const host=document.createElement("div");const root=createRoot(host);
  function Consumer({tab}:{tab:string}){const state=useAccountTransition(tab,"fixture");return <output>{`${state.transition?.id ?? "empty"}:${state.canVerify}:${state.turnActive}`}</output>;}
  try {
    await act(async()=>root.render(<Consumer tab="first"/>));
    await act(async()=>fixture.handlers.get("provider-account-transition")?.({payload:{tabId:"first",transition:{id:"delivered",providerId:"fixture"}}}));
    await act(async()=>resolve({transition:null,canVerify:true,turnActive:false}));
    expect(host.textContent).toBe("delivered:false:true");
    const retired=fixture.handlers.get("provider-account-transition")!;
    fixture.json.mockResolvedValue({transition:null,canVerify:false,turnActive:true});
    await act(async()=>root.render(<Consumer tab="second"/>));
    await act(async()=>retired({payload:{tabId:"first",transition:{id:"stale",providerId:"fixture"}}}));
    expect(host.textContent).toBe("empty:false:true");
  } finally {await act(async()=>root.unmount());}
  expect(fixture.handlers.size).toBe(0);
});
