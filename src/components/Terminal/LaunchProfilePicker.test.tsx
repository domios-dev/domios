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


it("renders the selected Gemini profile asset for an ACP tab and spawn picker", async () => {
  const {setAgentDescriptors} = await import("../../lib/agentDescriptorCache");
  const {AgentIcon} = await import("../AgentIcon");
  const {TabStatusLineModelPicker} = await import("./StatusLineModelPicker");
  const {readFileSync} = await import("node:fs");
  const manifest = JSON.parse(readFileSync("plugins/provider-acp/plugin.json", "utf8"));
  const profiles = manifest.capabilities.agentRuntime.launchProfiles;
  profiles["gemini-cli"].iconSvg = readFileSync(`plugins/provider-acp/${profiles["gemini-cli"].icon}`, "utf8");
  const provider = {id:"acp",display_name:"ACP",available_models:[],icon_svg:'<svg viewBox="0 0 24 24"><path d="M0 0"/></svg>',agent_runtime:{launchProfiles:profiles}} as unknown as ProviderDescriptor;
  setAgentDescriptors([provider]);
  const host = document.createElement("div"); const root = createRoot(host);
  const tab = {id:"gemini",title:"Gemini",tab_type:"Terminal",pty_id:null,cwd:null,agent_type:"acp",agent_runtime_config:{launchProfile:"gemini-cli"},agent_runtime_state:{state:"loaded",epoch:1,agentName:"Gemini CLI",binaryVersion:"1.2.3"}} as const;
  try {
    await act(async()=>root.render(<><AgentIcon tab={tab} providerId="acp"/><LaunchProfilePicker provider={provider} profiles={[]} selected="gemini-cli" onSelect={()=>{}} onRefresh={()=>{}} error={null}/><TabStatusLineModelPicker tab={tab} label="Gemini CLI"/></>));
    const logo = host.querySelector('[role="img"][aria-label="Gemini CLI · 1.2.3"]');
    expect(logo?.querySelector("svg")?.getAttribute("viewBox")).toBe("0 0 24 24");
    expect(logo?.querySelector("svg")?.outerHTML).toContain('d="M17 8H9V16H17V12H13"');
    expect(host.querySelector('[data-testid="status-harness-glyph"] svg')?.outerHTML).toContain('d="M17 8H9V16H17V12H13"');
    expect(host.querySelector('[role="img"][aria-label="gemini-cli"] svg')?.outerHTML).toContain('d="M17 8H9V16H17V12H13"');
  } finally {await act(async()=>root.unmount());setAgentDescriptors([]);}
});

it("falls back to the generic provider asset when a registry raster cannot load", async () => {
  const {setAgentDescriptors} = await import("../../lib/agentDescriptorCache");
  const {AgentIcon} = await import("../AgentIcon");
  setAgentDescriptors([{id:"acp",display_name:"ACP",icon_svg:'<svg viewBox="0 0 24 24"><path d="M1 2L3 4"/></svg>'} as ProviderDescriptor]);
  const host = document.createElement("div"); const root = createRoot(host);
  const tab = {id:"registry",title:"Registry",tab_type:"Terminal",pty_id:null,cwd:null,agent_type:"acp",agent_runtime_state:{state:"loaded",epoch:1,iconSvg:'<svg viewBox="0 0 24 24"><image href="data:image/png;base64,aW52YWxpZA=="/></svg>'}} as const;
  try {
    await act(async()=>root.render(<AgentIcon tab={tab} providerId="acp"/>));
    const image = host.querySelector("image"); expect(image).not.toBeNull();
    await act(async()=>image!.dispatchEvent(new Event("error")));
    expect(host.querySelector("image")).toBeNull();
    expect(host.querySelector("svg")?.outerHTML).toContain('d="M1 2L3 4"');
  } finally {await act(async()=>root.unmount());setAgentDescriptors([]);}
});
