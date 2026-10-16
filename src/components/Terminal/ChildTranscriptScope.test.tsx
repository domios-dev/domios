import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { reduceToolEvents, type NormalizedChatMessage } from "@codeterm/chat-core";
import { AgentActivityCard } from "../../../packages/chat-ui/src/toolcard/AgentActivityCard";
import { ChildTranscriptScope } from "./ChildTranscriptScope";
import type { ChildTranscriptPage } from "../../lib/childTranscript";

const bridge = vi.hoisted(()=>({invoke:vi.fn()}));
vi.mock("../../lib/bridge",()=>bridge);
(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT=true;
const authority={rootThreadId:"root",bindingNonce:"nonce",epoch:4};
const page:ChildTranscriptPage={...authority,child:{threadId:"child",parentThreadId:"root",ordinal:1,status:"completed",prompt:"Inspect inputs",model:"fixture",effort:"high"},items:[{id:"answer",type:"assistant",content:"Persisted child answer"}],nextCursor:null};
const row:NormalizedChatMessage={id:"spawn",type:"tool_call",content:"",toolKind:"agent",toolName:"agent_activity",toolInput:{agent_id:"child",agent_ordinal:1,agent_description:"Inspect inputs",agent_state:"completed"}};
const item=reduceToolEvents([row])[0];
if(item.kind!=="tool")throw new Error("Expected an agent card");
const event=item.event;
function Parent({epoch=4}:{epoch?:number}) {
  return <ChildTranscriptScope tabId="owned" authority={{...authority,epoch}}>
    <AgentActivityCard event={event} expanded onToggle={()=>{}} renderAction={()=>null} />
    <textarea aria-label="Parent composer" />
  </ChildTranscriptScope>;
}
async function mount() {
  const host=document.createElement("div");document.body.append(host);
  const root=createRoot(host);
  await act(async()=>root.render(<Parent />));
  const inspect=()=>[...host.querySelectorAll<HTMLButtonElement>("button")].find(button=>button.getAttribute("aria-label")?.startsWith("Inspect "))!;
  return {host,root,inspect,close:async()=>{await act(async()=>root.unmount());host.remove();}};
}

it("opens the provider child in the parent tab with only a history read and no child composer",async()=>{
  bridge.invoke.mockReset();bridge.invoke.mockResolvedValue(page);
  const view=await mount();
  try {
    await act(async()=>view.inspect().click());
    const transcript=view.host.querySelector('[aria-label="Subagent transcript"]')!;
    expect(transcript).not.toBeNull();
    expect(transcript.textContent).toContain("Persisted child answer");
    expect(transcript.querySelector("textarea,input,[contenteditable=true]")).toBeNull();
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    expect(bridge.invoke.mock.calls[0][0]).toBe("agent_runtime_child_history");
    expect(bridge.invoke.mock.calls[0][1]).toMatchObject({tabId:"owned",threadId:"child",request:authority});
    const back=[...transcript.querySelectorAll<HTMLButtonElement>("button")].find(button=>button.textContent==="Back to parent")!;
    await act(async()=>back.click());
    expect(view.host.querySelector('[aria-label="Subagent transcript"]')).toBeNull();
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
  }finally{await view.close();}
});

it("rejects foreign response rows and discards an in-flight read on root epoch change",async()=>{
  bridge.invoke.mockReset();bridge.invoke.mockResolvedValue({...page,child:{...page.child,threadId:"foreign"}});
  const view=await mount();
  try {
    await act(async()=>view.inspect().click());
    expect(view.host.querySelector('[role="alert"]')).not.toBeNull();
    expect(view.host.textContent).not.toContain("Persisted child answer");
    await act(async()=>view.root.render(<Parent epoch={5} />));
    let resolve!:(value:ChildTranscriptPage)=>void;
    bridge.invoke.mockImplementationOnce(()=>new Promise<ChildTranscriptPage>(done=>{resolve=done;}));
    await act(async()=>view.inspect().click());
    await act(async()=>view.root.render(<Parent epoch={6} />));
    await act(async()=>resolve({...page,epoch:5}));
    expect(view.host.querySelector('[aria-label="Subagent transcript"]')).toBeNull();
    expect(view.host.textContent).not.toContain("Persisted child answer");
  }finally{await view.close();}
});
