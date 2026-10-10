import { useEffect, useRef } from "react";
import { composerInsertionStore, type ContextCapsule } from "./composerInsertion";

/** Drains this tab's queued context into inline capsules, in order, without sending. */
export function useComposerInsertions(tabId: string, insertContext: (capsules: ContextCapsule[]) => Promise<void>) {
  const insertRef = useRef(insertContext);
  insertRef.current = insertContext;
  const chain = useRef<Promise<void>>(Promise.resolve());
  useEffect(() => {
    const drain = () => {
      const capsules = composerInsertionStore.take(tabId);
      if (!capsules.length) return;
      chain.current = chain.current.then(() => insertRef.current(capsules));
    };
    drain();
    return composerInsertionStore.subscribe(drain);
  }, [tabId]);
}
