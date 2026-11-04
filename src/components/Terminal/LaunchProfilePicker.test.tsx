import {act} from "react";
import {createRoot} from "react-dom/client";
import {expect,it,vi} from "vitest";
import {LaunchProfilePicker} from "./LaunchProfilePicker";
import {createTabsTransport} from "../../lib/transport/tabs";
import type {ProviderDescriptor} from "../../lib/types";
const bridge=vi.hoisted(()=>({invoke:vi.fn()}));
vi.mock("../../lib/bridge",()=>({invoke:bridge.invoke}));
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT=true;

it("keeps a confirmed install visibly in progress until its receipt arrives",async()=>{
  const host=document.createElement("div");document.body.append(host);const root=createRoot(host);
  let receipt!:(value:unknown)=>void;
  bridge.invoke.mockImplementation(()=>new Promise(resolve=>{receipt=resolve;}));
  const refresh=vi.fn();
  const provider={id:"fixture",agent_runtime:{launchProfiles:{selected:{binary:"npx",args:["opaque@1.0.0"],install:{package:"opaque@1.0.0"}}}}} as unknown as ProviderDescriptor;
  const button=(name:string)=>Array.from(host.querySelectorAll("button")).find(button=>button.textContent===name)!;
  try {
    await act(async()=>root.render(<LaunchProfilePicker provider={provider} profiles={[{id:"selected",install:"not_installed",auth:"unknown",canInstall:true,ownerConfigured:false}]} selected="selected" onSelect={()=>{}} onRefresh={refresh} error="Profile availability could not be checked"/>));
    await act(async()=>button("Install profile").click());
    await act(async()=>host.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click());
    await act(async()=>button("Confirm setup").click());
    expect(host.querySelector('[role="status"][aria-label="Profile installation"]')?.textContent).toContain("Installing");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(refresh).not.toHaveBeenCalled();
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    await act(async()=>receipt({profileId:"selected"}));
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(host.querySelector('[role="status"][aria-label="Profile installation"]')).toBeNull();
  } finally {await act(async()=>root.unmount());host.remove();}
});

it("waits for the install response without a client spinner deadline",async()=>{
  const json=vi.fn().mockResolvedValue({profileId:"selected"});
  const transport=createTabsTransport({json});
  await transport.provider_setup_profile({providerId:"fixture",request:{profileId:"selected",confirmed:true}});
  expect(json).toHaveBeenCalledWith("POST","/api/agents/providers/fixture/profiles",{profileId:"selected",confirmed:true},expect.objectContaining({timeoutMs:null}));
});
