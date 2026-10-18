import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { AgentInspectionProvider } from "@codeterm/chat-ui";
import { ComposerBackgroundWorkPanel, SubagentCapsules } from "./ComposerBackgroundWorkPanel";
import type { ComposerBackgroundWorkItem } from "./composerBackgroundWorkModel";

(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT=true;
it("opens a child from a named capsule and exposes every overflow child", async()=>{
  const host=document.createElement("div");
  const root=createRoot(host);
  const inspect=vi.fn();
  const items:ComposerBackgroundWorkItem[]=Array.from({length:5},(_,index)=>({id:String(index),kind:"agent",name:`Worker ${index}`,threadId:`child-${index}`,transcriptThreadId:`child-${index}`,startedAtMs:0,elapsed:"20s",status:"running",model:"fixture"}));
  try {
    await act(async()=>root.render(<AgentInspectionProvider onInspect={inspect}><SubagentCapsules items={items} /></AgentInspectionProvider>));
    const first=host.querySelector<HTMLButtonElement>('button[aria-label="Open transcript for Worker 0"]')!;
    expect(first).not.toBeNull();
    await act(async()=>first.click());
    expect(inspect).toHaveBeenCalledExactlyOnceWith("child-0");
    expect(host.querySelector('button[aria-label="Open transcript for Worker 4"]')).toBeNull();
    await act(async()=>host.querySelector<HTMLButtonElement>('button[aria-label="Show 2 more subagents"]')!.click());
    await act(async()=>host.querySelector<HTMLButtonElement>('button[aria-label="Open transcript for Worker 4"]')!.click());
    expect(inspect).toHaveBeenLastCalledWith("child-4");
  } finally { await act(async()=>root.unmount()); }
});

it("keeps stop single-flight, displays refusal, and retains details behind expansion", async()=>{
  const host=document.createElement("div");
  const root=createRoot(host);
  const item:ComposerBackgroundWorkItem={id:"job",kind:"terminal",name:"fixture.ps1",threadId:"root",startedAtMs:0,elapsed:"1m",status:"running",processId:"process",command:"opaque command fixture",outputTail:"opaque output fixture"};
  let rejectStop!:(reason:Error)=>void;
  const stop=vi.fn(()=>new Promise<void>((_,reject)=>{rejectStop=reject;}));
  try {
    await act(async()=>root.render(<ComposerBackgroundWorkPanel items={[item]} onStop={stop} />));
    expect(host.querySelector('[role="region"]')).toBeNull();
    const stopButton=host.querySelector<HTMLButtonElement>('button[aria-label="Stop fixture.ps1"]')!;
    await act(async()=>{stopButton.click();stopButton.click();});
    expect(stop).toHaveBeenCalledExactlyOnceWith(item);
    expect(stopButton.disabled).toBe(true);
    expect(stopButton.getAttribute("aria-busy")).toBe("true");
    await act(async()=>rejectStop(new Error("fixture refusal")));
    expect(host.querySelector('[role="alert"]')).not.toBeNull();
    expect(stopButton.disabled).toBe(false);
    const details=host.querySelector<HTMLButtonElement>('button[aria-label="Details for fixture.ps1"]')!;
    await act(async()=>details.click());
    expect(host.querySelector('[role="region"] code')?.textContent).toBe(item.command);
    expect(host.querySelector('[role="region"] pre')?.textContent).toBe(item.outputTail);
  } finally { await act(async()=>root.unmount()); }
});
