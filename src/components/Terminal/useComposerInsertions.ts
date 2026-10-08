import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";
import { applyComposerInsertions, composerInsertionStore } from "./composerInsertion";

/** Adds queued panel context to this tab's draft and attachment chips, then focuses the composer without sending. */
export function useComposerInsertions(tabId: string, { draft, setDraft, addAttachments, textareaRef, focus }: {
  draft: string;
  setDraft: (value: string) => void;
  addAttachments: (paths: string[]) => void;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  focus: () => void;
}) {
  const draftRef = useRef(draft);
  draftRef.current = draft;
  const caretToEnd = useRef(false);
  useEffect(() => {
    const drain = () => {
      const insertions = composerInsertionStore.take(tabId);
      if (!insertions.length) return;
      const result = applyComposerInsertions(draftRef.current, insertions);
      if (result.attachments.length) addAttachments(result.attachments);
      if (result.draft !== draftRef.current) {
        caretToEnd.current = true;
        draftRef.current = result.draft;
        setDraft(result.draft);
      }
      focus();
    };
    drain();
    return composerInsertionStore.subscribe(drain);
  }, [tabId, addAttachments, setDraft, focus]);
  useLayoutEffect(() => {
    const element = textareaRef.current;
    if (!caretToEnd.current || !element) return;
    caretToEnd.current = false;
    element.setSelectionRange(element.value.length, element.value.length);
    element.scrollTop = element.scrollHeight;
  }, [draft, textareaRef]);
}
