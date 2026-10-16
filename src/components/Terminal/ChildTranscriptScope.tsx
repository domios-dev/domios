import { useEffect, useRef, useState, type ReactNode } from "react";
import { AgentInspectionProvider, MessageBubble } from "@codeterm/chat-ui";
import { childTranscriptKey, type ChildTranscriptAuthority } from "../../lib/childTranscript";
import { useChildTranscript } from "./useChildTranscript";

function ChildTranscript({tabId,threadId,authority,onClose}: {tabId:string;threadId:string;authority:ChildTranscriptAuthority;onClose:()=>void}) {
  const transcript = useChildTranscript(tabId,threadId,authority);
  const back = useRef<HTMLButtonElement>(null);
  useEffect(() => back.current?.focus(), []);
  return <section aria-label="Subagent transcript" className="flex h-full min-h-0 flex-col" style={{background:"var(--ct-bg)",color:"var(--ct-fg)"}}>
    <header className="flex shrink-0 items-center gap-3 border-b p-3" style={{borderColor:"var(--ct-border)"}}>
      <button ref={back} type="button" onClick={onClose}>Back to parent</button>
      <h2 className="min-w-0 flex-1 truncate text-sm">{transcript.child?.prompt ?? "Subagent transcript"}</h2>
      <span className="text-xs">Read only</span>
      <button type="button" disabled={transcript.loading} onClick={transcript.refresh}>Refresh transcript</button>
    </header>
    {transcript.child && <p className="m-0 px-3 py-1 text-xs">{[transcript.child.model,transcript.child.effort,transcript.child.status].filter(Boolean).join(" · ")}</p>}
    {transcript.error && <p role="alert" className="p-3">{transcript.error}</p>}
    {transcript.loading && <p role="status" className="p-3">Loading transcript</p>}
    <div role="log" aria-label="Subagent messages" aria-busy={transcript.loading} className="min-h-0 flex-1 overflow-auto p-3">
      {transcript.nextCursor && <button type="button" disabled={transcript.loading} onClick={transcript.older}>Load older messages</button>}
      {transcript.items.map(item=><MessageBubble key={item.id} message={{...item,interactive:[]}} />)}
      {!transcript.loading && !transcript.error && !transcript.items.length && <p>No messages yet</p>}
    </div>
  </section>;
}

export function ChildTranscriptScope({tabId,authority,children}: {tabId:string;authority:ChildTranscriptAuthority|null;children:ReactNode}) {
  const [selection,setSelection] = useState<{key:string;thread:string}|null>(null);
  const returnFocus = useRef<HTMLElement | null>(null);
  const key = authority ? childTranscriptKey(tabId,authority) : null;
  const active = selection?.key === key && authority ? selection : null;
  useEffect(() => { if (!active) returnFocus.current?.focus(); }, [active]);
  return <AgentInspectionProvider onInspect={authority ? thread=>{
    if (!active) returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setSelection({key:key!,thread});
  } : undefined}>
    <div className="h-full min-h-0">
      <div hidden={Boolean(active)} inert={Boolean(active)} className="h-full min-h-0">{children}</div>
      {active && authority && <ChildTranscript key={JSON.stringify([active.key,active.thread])} tabId={tabId} threadId={active.thread} authority={authority} onClose={()=>setSelection(null)} />}
    </div>
  </AgentInspectionProvider>;
}
