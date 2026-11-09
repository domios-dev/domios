import {act} from "react";
import {createRoot} from "react-dom/client";
import {expect,it,vi} from "vitest";
import {LaunchProfilePicker} from "./LaunchProfilePicker";
import {useLaunchProfiles, type LaunchProfileState, type ProfileSetupJob} from "./useLaunchProfiles";
import {EVENT_STREAM_READY} from "../../lib/eventStreamReady";
import {fireResync} from "../../lib/resync";
import {createTabsTransport} from "../../lib/transport/tabs";
import type {ProviderDescriptor} from "../../lib/types";
const bridge=vi.hoisted(()=>({invoke:vi.fn(),listen:vi.fn()}));
vi.mock("../../lib/bridge",()=>bridge);
const helper=vi.hoisted(()=>({spawnAiHelper:vi.fn()}));
vi.mock("../../lib/aiHelperSpawn",async original=>({...(await original<typeof import("../../lib/aiHelperSpawn")>()),spawnAiHelper:helper.spawnAiHelper}));
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT=true;

it("keeps an accepted install visibly in progress until its catalog job completes",async()=>{
  const host=document.createElement("div");document.body.append(host);const root=createRoot(host);
  let receipt!:(value:unknown)=>void;
  bridge.invoke.mockImplementation(()=>new Promise(resolve=>{receipt=resolve;}));
  const refresh=vi.fn();
  const provider={id:"fixture",agent_runtime:{launchProfiles:{selected:{binary:"npx",args:["opaque@1.0.0"],install:{package:"opaque@1.0.0"}}}}} as unknown as ProviderDescriptor;
  const job = {jobId:"job",providerId:"fixture",profileId:"selected",state:"accepted" as const};
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
    await act(async()=>receipt(job));
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(host.querySelector('[role="status"][aria-label="Profile installation"]')).not.toBeNull();
    await act(async()=>root.render(<LaunchProfilePicker provider={provider} profiles={[{id:"selected",install:"installed",auth:"unknown",canInstall:true,ownerConfigured:false,setup:{...job,state:"installed"}}]} selected="selected" onSelect={()=>{}} onRefresh={refresh} error={null}/>));
    expect(host.querySelector('[role="status"][aria-label="Profile installation"]')).toBeNull();
    expect(host.querySelector('[role="radio"][aria-checked="true"]')?.textContent).toContain("Installed");
    expect(host.querySelector('[role="alert"]')).toBeNull();
    await act(async()=>root.render(<LaunchProfilePicker provider={provider} profiles={[{id:"selected",install:"installed",auth:"unknown",canInstall:true,ownerConfigured:false}]} selected="selected" onSelect={()=>{}} onRefresh={refresh} error={null}/>));
    expect(host.querySelector('[role="status"][aria-label="Profile installation"]')).toBeNull();
  } finally {await act(async()=>root.unmount());host.remove();}
});

it("restores active setup and shows its actual terminal failure",async()=>{
  const host=document.createElement("div");document.body.append(host);const root=createRoot(host);
  const provider={id:"fixture",agent_runtime:{launchProfiles:{selected:{binary:"npx",args:[],install:{package:"opaque@1.0.0"}}}}} as unknown as ProviderDescriptor;
  const profile={id:"selected",install:"not_installed" as const,auth:"unknown" as const,canInstall:true,ownerConfigured:false};
  const job={jobId:"reopened",providerId:"fixture",profileId:"selected",state:"installing" as const};
  try {
    await act(async()=>root.render(<LaunchProfilePicker provider={provider} profiles={[{...profile,setup:job}]} selected="selected" onSelect={()=>{}} onRefresh={()=>{}} error={null}/>));
    expect(host.querySelector('[aria-label="Profile installation"]')).not.toBeNull();
    await act(async()=>root.render(<LaunchProfilePicker provider={provider} profiles={[{...profile,setup:{...job,state:"failed",error:"Checksum mismatch"}}]} selected="selected" onSelect={()=>{}} onRefresh={()=>{}} error={null}/>));
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("Checksum mismatch");
    expect(host.querySelector('[aria-label="Profile installation"]')).toBeNull();
  } finally {await act(async()=>root.unmount());host.remove();}
});

it("reconciles a lost acceptance response with the installed job",async()=>{
  const host=document.createElement("div");document.body.append(host);const root=createRoot(host);
  bridge.invoke.mockRejectedValue(new Error("Request disconnected"));
  const provider={id:"fixture",agent_runtime:{launchProfiles:{selected:{binary:"npx",args:[],install:{package:"opaque@1.0.0"}}}}} as unknown as ProviderDescriptor;
  const profile={id:"selected",install:"not_installed" as const,auth:"unknown" as const,canInstall:true,ownerConfigured:false};
  const button=(name:string)=>Array.from(host.querySelectorAll("button")).find(button=>button.textContent===name)!;
  try {
    await act(async()=>root.render(<LaunchProfilePicker provider={provider} profiles={[profile]} selected="selected" onSelect={()=>{}} onRefresh={()=>{}} error={null}/>));
    await act(async()=>button("Install profile").click());
    await act(async()=>host.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click());
    await act(async()=>button("Confirm setup").click());
    await act(async()=>root.render(<LaunchProfilePicker provider={provider} profiles={[{...profile,install:"installed",setup:{jobId:"delivered",providerId:"fixture",profileId:"selected",state:"installed"}}]} selected="selected" onSelect={()=>{}} onRefresh={()=>{}} error={null}/>));
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.querySelector('[role="radio"][aria-checked="true"]')?.textContent).toContain("Installed");
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
    expect(logo?.querySelectorAll("linearGradient").length).toBe(3);
    const ids = [...host.querySelectorAll("linearGradient")].map(gradient => gradient.id);
    expect(new Set(ids).size).toBe(9);
    for (const svg of host.querySelectorAll("svg")) {
      for (const path of svg.querySelectorAll('path[fill^="url("]')) {
        const id = path.getAttribute("fill")!.slice(5,-1);
        expect([...svg.querySelectorAll("linearGradient")].some(gradient => gradient.id === id)).toBe(true);
      }
    }
    expect(logo?.querySelector("svg")?.outerHTML).toContain('fill="#3186FF"');
    expect(host.querySelector('[data-testid="status-harness-glyph"] svg')?.outerHTML).toContain('fill="#3186FF"');
    expect(host.querySelector('[role="img"][aria-label="gemini-cli"] svg')?.outerHTML).toContain('fill="#3186FF"');
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

it("subscribes before reading and ignores a stale catalog after setup completion or reconnect",async()=>{
  let event!:(event:{payload:ProfileSetupJob})=>void;
  const stop=vi.fn();
  const reads: ((profiles:LaunchProfileState[])=>void)[]=[];
  bridge.listen.mockImplementation((_name:string,handler:typeof event)=>{event=handler;return Promise.resolve(stop);});
  bridge.invoke.mockImplementation(()=>new Promise<LaunchProfileState[]>(resolve=>reads.push(resolve)));
  const provider={id:"fixture",agent_runtime:{launchProfiles:{}}} as ProviderDescriptor;
  function View(){const state=useLaunchProfiles(provider);return <div>{JSON.stringify(state.profiles)}</div>;}
  const host=document.createElement("div");document.body.append(host);const root=createRoot(host);
  const job={jobId:"job",providerId:"fixture",profileId:"selected",state:"installed" as const};
  const profile:LaunchProfileState={id:"selected",install:"installed",auth:"unknown",canInstall:true,ownerConfigured:false,setup:job};
  try {
    await act(async()=>root.render(<View/>));
    expect(bridge.listen).toHaveBeenCalledWith("provider-profile-setup",expect.any(Function));
    expect(reads).toHaveLength(1);
    await act(async()=>event({payload:job}));
    expect(reads).toHaveLength(2);
    await act(async()=>reads[1]([profile]));
    await act(async()=>reads[0]([{...profile,install:"not_installed",setup:{...job,state:"installing"}}]));
    expect(host.textContent).toContain('"state":"installed"');
    expect(host.textContent).not.toContain('"state":"installing"');
    await act(async()=>window.dispatchEvent(new Event(EVENT_STREAM_READY)));
    expect(reads).toHaveLength(3);
    await act(async()=>reads[2]([]));
    expect(host.textContent).toBe("[]");
    await act(async()=>fireResync());
    expect(reads).toHaveLength(4);
    await act(async()=>reads[3]([profile]));
    expect(host.textContent).toContain('"state":"installed"');
  } finally {await act(async()=>root.unmount());host.remove();}
  expect(stop).toHaveBeenCalledTimes(1);
});

it("with no ACP agent set up, says so in one line and offers the General agent once",async()=>{
  const host=document.createElement("div");document.body.append(host);const root=createRoot(host);
  helper.spawnAiHelper.mockReset().mockResolvedValue({tabId:"general",workspaceId:"w",deliveryState:"confirmed"});
  const provider={id:"acp",agent_runtime:{launchProfiles:{"gemini-cli":{binary:"gemini",args:["--acp"]},"deepseek-harness":{binary:"npx",args:[]}}}} as unknown as ProviderDescriptor;
  const profiles:LaunchProfileState[]=[
    {id:"gemini-cli",install:"not_installed",auth:"unknown",canInstall:true,ownerConfigured:false},
    {id:"deepseek-harness",install:"not_installed",auth:"unknown",canInstall:true,ownerConfigured:false},
  ];
  const buttons=()=>Array.from(host.querySelectorAll("button")).map(button=>button.textContent);
  try {
    await act(async()=>root.render(<LaunchProfilePicker provider={provider} profiles={profiles} selected="" onSelect={()=>{}} onRefresh={()=>{}} error={null}/>));
    expect(host.textContent).toContain("No ACP agents set up yet.");
    expect(buttons().filter(name=>name?.includes("General agent"))).toEqual(["Ask the General agent"]);
    await act(async()=>Array.from(host.querySelectorAll("button")).find(button=>button.textContent==="Ask the General agent")!.click());
    expect(helper.spawnAiHelper).toHaveBeenCalledTimes(1);
    expect(helper.spawnAiHelper.mock.calls[0][0].task).toContain("Explain in one or two plain sentences what ACP is");

    await act(async()=>root.render(<LaunchProfilePicker provider={provider} profiles={[{...profiles[0],install:"installed",auth:"authenticated"},profiles[1]]} selected="gemini-cli" onSelect={()=>{}} onRefresh={()=>{}} error={null}/>));
    expect(host.textContent).not.toContain("No ACP agents set up yet.");
    expect(host.querySelector('[role="radio"][aria-checked="true"]')?.textContent).toContain("Ready");
    await act(async()=>Array.from(host.querySelectorAll("button")).find(button=>button.textContent==="Set up with the General agent")!.click());
    expect(helper.spawnAiHelper.mock.calls[1][0].task).toContain("Connect an ACP agent: gemini-cli.");
  } finally {await act(async()=>root.unmount());host.remove();}
});

it("shows install and sign-in state as one quiet word per profile",async()=>{
  const {profileStateLabel}=await import("./LaunchProfilePicker");
  const base:LaunchProfileState={id:"p",install:"installed",auth:"unknown",canInstall:true,ownerConfigured:false};
  expect(profileStateLabel({...base,auth:"authenticated"},undefined)).toBe("Ready");
  expect(profileStateLabel({...base,auth:"needs_auth"},undefined)).toBe("Sign in needed");
  expect(profileStateLabel({...base,install:"not_installed"},undefined)).toBe("Not installed");
  expect(profileStateLabel({...base,install:"not_installed",canInstall:false,ownerConfigured:true},undefined)).toBe("Not set up");
  expect(profileStateLabel(base,{jobId:"j",providerId:"acp",profileId:"p",state:"installing"})).toBe("Installing…");
  expect(profileStateLabel(base,{jobId:"j",providerId:"acp",profileId:"p",state:"failed"})).toBe("Setup failed");
});
