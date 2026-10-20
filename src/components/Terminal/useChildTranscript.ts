import { useEffect, useRef, useState } from "react";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { invoke } from "../../lib/bridge";
import { acceptChildPage, prependChildPage, type ChildTranscriptAuthority, type ChildTranscriptPage, type ChildMetadata } from "../../lib/childTranscript";

export function useChildTranscript(tabId: string, threadId: string, authority: ChildTranscriptAuthority) {
  const {rootThreadId, bindingNonce, epoch} = authority;
  const [request, setRequest] = useState<{cursor?:string; revision:number}>({revision:0});
  const [items, setItems] = useState<NormalizedChatMessage[]>([]);
  const [child, setChild] = useState<ChildMetadata | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const pageAuthority = useRef<ChildTranscriptAuthority | null>(null);
  const {cursor, revision} = request;
  useEffect(() => {
    const controller = new AbortController();
    const binding = {rootThreadId, bindingNonce, epoch};
    setLoading(true);
    setError(null);
    if (!cursor) { setItems([]); setChild(null); }
    void invoke<ChildTranscriptAuthority>("agent_runtime_child_authority", { tabId, signal: controller.signal }).then(async current => {
      if (controller.signal.aborted) return;
      if (current.rootThreadId !== binding.rootThreadId) throw new Error("Parent session changed");
      const sameBinding = current.bindingNonce === pageAuthority.current?.bindingNonce && current.epoch === pageAuthority.current?.epoch;
      if (cursor && !sameBinding) throw new Error("Parent runtime changed; refresh the transcript");
      const page = await invoke<ChildTranscriptPage>("agent_runtime_child_history", {
        tabId, threadId, request:{...current,...(cursor ? {cursor} : {})}, signal:controller.signal,
      });
      if (controller.signal.aborted) return;
      if (!acceptChildPage(current, threadId, page)) throw new Error("Subagent transcript binding changed");
      pageAuthority.current = current;
      setChild(page.child);
      setItems(current => cursor ? prependChildPage(current,page.items) : page.items);
      setNextCursor(page.nextCursor);
    }).catch((reason:unknown) => {
      if (!controller.signal.aborted) { setError(reason instanceof Error ? reason.message : String(reason)); setItems([]); setChild(null); }
    }).finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [tabId, threadId, rootThreadId, bindingNonce, epoch, cursor, revision]);
  return {items,child,loading,error,nextCursor,
    older:() => { if (nextCursor && !loading) setRequest(previous=>({cursor:nextCursor,revision:previous.revision+1})); },
    refresh:() => { if (!loading) setRequest(previous=>({revision:previous.revision+1})); },
  };
}
