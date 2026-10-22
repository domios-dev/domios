import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { AgentInspectionProvider } from "@codeterm/chat-ui";
import { ComposerBackgroundWorkPanel } from "./ComposerBackgroundWorkPanel";
import type { ComposerBackgroundWorkItem } from "./composerBackgroundWorkModel";

(globalThis as {IS_REACT_ACT_ENVIRONMENT?:boolean}).IS_REACT_ACT_ENVIRONMENT=true;
it("lists every internal child with its harness label and transcript access", async()=>{
  const host=document.createElement("div");
  const root=createRoot(host);
  const inspect=vi.fn();
  const items:ComposerBackgroundWorkItem[]=Array.from({length:5},(_,index)=>({id:String(index),kind:"agent",name:`Worker ${index}`,threadId:`child-${index}`,transcriptThreadId:`child-${index}`,startedAtMs:0,elapsed:"20s",status:"running",model:"fixture"}));
  try {
    await act(async()=>root.render(<AgentInspectionProvider onInspect={inspect}><ComposerBackgroundWorkPanel items={items} onStop={async()=>{}} internalAgentLabel="Internal agent of Fixture Harness" /></AgentInspectionProvider>));
    const first=host.querySelector<HTMLButtonElement>('button[aria-label="Open transcript for Worker 0, Internal agent of Fixture Harness"]')!;
    expect(first).not.toBeNull();
    await act(async()=>first.click());
    expect(inspect).toHaveBeenCalledExactlyOnceWith("child-0");
    expect(host.querySelectorAll('button[aria-label^="Open transcript for "]')).toHaveLength(5);
    await act(async()=>host.querySelector<HTMLButtonElement>('button[aria-label="Open transcript for Worker 4, Internal agent of Fixture Harness"]')!.click());
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
    await act(async()=>root.render(<ComposerBackgroundWorkPanel items={[item]} onStop={stop} internalAgentLabel="Internal agent of Fixture Harness" />));
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
