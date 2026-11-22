import { useLayoutEffect, useRef, useState, type RefObject } from "react";
import { canonicalMentionSlice, snapMentionSelection, type MentionDraft } from "../composerMentionTokens";
import { DictationSessionModel, type DictationEdit, type DictationSelection, type DictationUpdate } from "./dictationSessionModel";

export interface DictationDraftPort {
  text: string;
  edit(text: string): void;
  current(): string;
  start(session: string, draft: MentionDraft, start: number, end: number): void;
  startAt(session: string, canonical: string, start: number, end: number): void;
  update(update: DictationUpdate, draft: MentionDraft): void;
  cancel(draft: MentionDraft): void;
  noteSelection(draft: MentionDraft, inputType: string): void;
}

function sourcePosition(draft: MentionDraft, display: number) {
  return canonicalMentionSlice(draft, 0, display).length;
}

function displayPosition(draft: MentionDraft, source: number) {
  let result = source;
  for (const token of draft.tokens) {
    if (token.sourceStart >= source) break;
    if (token.sourceEnd > source) return token.start;
    result += token.end - token.start - (token.sourceEnd - token.sourceStart);
  }
  return result;
}

function rebaseSelection(position: number, edit: DictationEdit) {
  if (position < edit.start) return position;
  if (position <= edit.end) return edit.start + edit.text.length;
  return position + edit.text.length - (edit.end - edit.start);
}

export function useDictationDraft(persisted: string, persist: (text: string) => void,
  textarea: RefObject<HTMLTextAreaElement | null>, scope: string, composition: { active: boolean }):
  DictationDraftPort & { restoreSelection(draft: MentionDraft): void } {
  const active = useRef<{ scope: string; model: DictationSessionModel } | null>(null);
  const [preview, setPreview] = useState<{ scope: string; text: string; base: string } | null>(null);
  const selection = useRef<{ start: number; end: number; direction: "forward" | "backward" | "none" | null } | null>(null);
  const editTarget = useRef<(DictationSelection & { draft: string }) | null>(null);
  const lastPersisted = useRef(persisted);
  lastPersisted.current = persisted;
  const write = (value: string) => {
    if (value === lastPersisted.current) return;
    lastPersisted.current = value;
    persist(value);
  };
  const publish = (model: DictationSessionModel, finished: boolean) => {
    const durable = model.durable;
    write(durable);
    setPreview(finished ? null : { scope, text: model.text, base: durable });
    if (finished) active.current = null;
  };
  function sync(model: DictationSessionModel, draft: MentionDraft) {
    if (draft.canonical !== model.text && lastPersisted.current !== model.durable) {
      model.cancel();
      active.current = null;
      setPreview(null);
      return false;
    }
    return true;
  }
  function mapSelection(draft: MentionDraft, edit: DictationEdit) {
    const editor = textarea.current;
    if (!editor || editor.value !== draft.text) return;
    const range = snapMentionSelection(draft.tokens, editor.selectionStart, editor.selectionEnd);
    const prior = selection.current ?? { start: sourcePosition(draft, range.start), end: sourcePosition(draft, range.end),
      direction: editor.selectionDirection };
    selection.current = { start: rebaseSelection(prior.start, edit), end: rebaseSelection(prior.end, edit), direction: prior.direction };
  }

  return {
    text: preview?.scope === scope && preview.base === persisted ? preview.text : persisted,
    current: () => lastPersisted.current,
    startAt(session, canonical, start, end) {
      active.current = { scope, model: new DictationSessionModel(session, canonical, start, end) };
    },
    start(session, draft, start, end) {
      const range = snapMentionSelection(draft.tokens, start, end);
      active.current = { scope, model: new DictationSessionModel(session, draft.canonical,
        sourcePosition(draft, range.start), sourcePosition(draft, range.end)) };
    },
    noteSelection(draft, inputType) {
      const editor = textarea.current;
      if (active.current?.scope !== scope || !editor || editor.value !== draft.text) return;
      if (composition.active && editTarget.current) return;
      editTarget.current = { start: sourcePosition(draft, editor.selectionStart),
        end: sourcePosition(draft, editor.selectionEnd), inputType, draft: draft.canonical };
    },
    edit(text) {
      const model = active.current?.scope === scope ? active.current.model : null;
      if (model && composition.active) {
        setPreview({ scope, text, base: model.durable });
        return;
      }
      const target = editTarget.current;
      editTarget.current = null;
      selection.current = null;
      if (!model) return write(text);
      model.edit(text, target?.draft === model.text ? target : undefined);
      publish(model, false);
    },
    update(update, draft) {
      const model = active.current?.scope === scope ? active.current.model : null;
      if (!model || !sync(model, draft)) return;
      const edit = model.update(update);
      if (edit === null) return;
      if (edit && draft.canonical.slice(edit.start, edit.end) !== edit.text) mapSelection(draft, edit);
      if (edit || update.final) publish(model, update.final);
    },
    cancel(draft) {
      const model = active.current?.scope === scope ? active.current.model : null;
      if (!model || !sync(model, draft)) return;
      const edit = model.cancel();
      if (edit) mapSelection(draft, edit);
      publish(model, true);
    },
    restoreSelection(draft) {
      const pending = selection.current;
      const editor = textarea.current;
      if (!pending || !editor) return;
      selection.current = null;
      editor.setSelectionRange(displayPosition(draft, pending.start), displayPosition(draft, pending.end), pending.direction ?? "none");
    },
  };
}

export function useDictationSelection(port: ReturnType<typeof useDictationDraft>, draft: MentionDraft) {
  useLayoutEffect(() => { port.restoreSelection(draft); });
}
