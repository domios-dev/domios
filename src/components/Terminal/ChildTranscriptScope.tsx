import { useEffect, useRef, useState, type ReactNode } from "react";
import { ArrowLeft, RefreshCw } from "lucide-react";
import { reduceToolEvents } from "@codeterm/chat-core";
import { AgentInspectionProvider, MessageBubble } from "@codeterm/chat-ui";
import { childTranscriptKey, type ChildTranscriptAuthority } from "../../lib/childTranscript";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { Button } from "../shared/Button";
import { useChildTranscript } from "./useChildTranscript";
import { childTranscriptTitle, childTranscriptViewState, subagentTone } from "./childTranscriptViewModel";
import { RegionLoader } from "../shared/RegionLoader";

function ChildTranscript({ tabId, threadId, authority, onClose }: { tabId: string; threadId: string; authority: ChildTranscriptAuthority; onClose: () => void }) {
  const transcript = useChildTranscript(tabId, threadId, authority);
  const back = useRef<HTMLButtonElement>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const anchor = useRef<{ height: number; top: number } | null>(null);
  const rows = reduceToolEvents(transcript.items);
  const state = childTranscriptViewState({ ...transcript, itemCount: rows.length });
  useEffect(() => back.current?.focus(), []);
  useEffect(() => {
    const element = scroll.current;
    if (transcript.loading || !element) return;
    if (anchor.current) {
      element.scrollTop = anchor.current.top + element.scrollHeight - anchor.current.height;
      anchor.current = null;
    } else element.scrollTop = element.scrollHeight;
  }, [transcript.loading]);
  const older = () => {
    if (!transcript.nextCursor || transcript.loading) return;
    if (scroll.current) anchor.current = { height: scroll.current.scrollHeight, top: scroll.current.scrollTop };
    transcript.older();
  };
  return <section aria-label="Subagent transcript" className="ct-child-transcript">
    <header className="ct-child-transcript__header">
      <Button ref={back} variant="icon" aria-label="Back to parent" title="Back to parent (Esc)" onClick={onClose}><ArrowLeft size={18} aria-hidden="true" /></Button>
      <div className="ct-child-transcript__identity">
        <h2>{childTranscriptTitle(transcript.child)}</h2>
        {transcript.child?.prompt && <p title={transcript.child.prompt}>{transcript.child.prompt}</p>}
        <div className="ct-child-transcript__chips">
          {transcript.child && [transcript.child.model, transcript.child.effort, transcript.child.status].filter(Boolean).map((value, index) => <span key={index} className="ct-child-transcript__chip" data-tone={subagentTone(value ?? undefined)}>{value}</span>)}
        </div>
      </div>
      <span className="ct-child-transcript__chip">Read only</span>
      <Button variant="icon" aria-label="Refresh transcript" title="Refresh transcript" disabled={transcript.loading} onClick={transcript.refresh}><RefreshCw size={16} aria-hidden="true" /></Button>
    </header>
    {transcript.error && <div role="alert" className="ct-child-transcript__notice"><InlineErrorNotice error={transcript.error} operation="loading the subagent transcript" /></div>}
    <div ref={scroll} role="log" aria-label="Subagent messages" aria-busy={transcript.loading} className="ct-child-transcript__messages" onScroll={event => { if (event.currentTarget.scrollTop <= 32) older(); }}>
      <div className="ct-child-transcript__lane">
        {transcript.nextCursor && <div className="ct-child-transcript__older"><Button size="compact" disabled={transcript.loading} onClick={older}>Load older messages</Button></div>}
        {rows.map(item => {
          const message = item.kind === "tool" ? item.event.message : item.message;
          return <MessageBubble key={message.id} message={{ ...message, interactive: [] }} item={item.kind === "tool" ? item : undefined} />;
        })}
      </div>
      {state === "loading" && <RegionLoader label="Loading transcript" className="min-h-40" />}
      {state === "empty" && <p role="status" className="ct-child-transcript__empty">This sub-agent has not produced output</p>}
    </div>
  </section>;
}

export function ChildTranscriptScope({ tabId, authority, children }: { tabId: string; authority: ChildTranscriptAuthority | null; children: ReactNode }) {
  const [selection, setSelection] = useState<{ key: string; thread: string } | null>(null);
  const returnFocus = useRef<HTMLElement | null>(null);
  const key = authority ? childTranscriptKey(tabId, authority) : null;
  const active = selection?.key === key && authority ? selection : null;
  useEffect(() => {
    if (!active) { returnFocus.current?.focus(); return; }
    const close = (event: KeyboardEvent) => {
      if (event.code !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      setSelection(null);
    };
    document.addEventListener("keydown", close);
    return () => document.removeEventListener("keydown", close);
  }, [active]);
  useEffect(() => { setSelection(null); }, [key]);
  return <AgentInspectionProvider onInspect={authority ? thread => {
    if (!active) returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setSelection({ key: key!, thread });
  } : undefined}>
    <div className="h-full min-h-0">
      <div hidden={Boolean(active)} inert={Boolean(active)} className="h-full min-h-0">{children}</div>
      {active && authority && <ChildTranscript key={JSON.stringify([active.key, active.thread])} tabId={tabId} threadId={active.thread} authority={authority} onClose={() => setSelection(null)} />}
    </div>
  </AgentInspectionProvider>;
}
