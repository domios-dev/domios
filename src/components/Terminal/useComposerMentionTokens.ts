import { useLayoutEffect, useRef, useState, type ClipboardEvent, type InputEvent, type KeyboardEvent, type RefObject } from "react";
import { DOMIOS_TEXT_MIME } from "@codeterm/chat-core";
import { referenceMarkers } from "../../../packages/chat-ui/src/codeterm/referenceMarkers";
import type { ComposerMention, MentionQuery } from "../../lib/composerMentions";
import { useMuxStateStore } from "../../store/multiplexer/selectorStore";
import { replaceTextareaRange } from "./useComposerCapsules";
import { canonicalMentionSlice, mentionArrowTarget, mentionDeletionRange, MentionTokenModel, normalizePastedMentions, snapMentionSelection } from "./composerMentionTokens";

export function useComposerMentionTokens(canonical: string, setCanonical: (text: string) => void, textarea: RefObject<HTMLTextAreaElement | null>) {
  const mux = useMuxStateStore();
  const model = useRef<MentionTokenModel | null>(null);
  if (!model.current) model.current = new MentionTokenModel(canonical, id => mux.getState().tabs[id]?.title ?? "Tab");
  const draft = model.current.sync(canonical);
  const [announcement, announce] = useState("");
  const change = (text: string, inputType = "") => setCanonical(model.current!.change(text, inputType).canonical);
  const snapSelection = (el: HTMLTextAreaElement) => {
    const range = snapMentionSelection(model.current!.current.tokens, el.selectionStart, el.selectionEnd);
    if (range.start !== el.selectionStart || range.end !== el.selectionEnd) el.setSelectionRange(range.start, range.end, el.selectionDirection ?? "none");
  };
  const replace = (el: HTMLTextAreaElement, start: number, end: number, text: string, label?: { id: string; title: string }) => {
    const before = model.current!.current;
    const next = model.current!.prepare(start, end, text, label);
    const insertedLength = next.text.length - (before.text.length - (end - start));
    const insertion = next.text.slice(start, start + insertedLength);
    replaceTextareaRange(el, start, end, insertion);
    if (model.current!.pending) change(next.text);
    el.setSelectionRange(start + insertion.length, start + insertion.length);
    const added = next.tokens.filter(token => token.start >= start && token.end <= start + insertion.length);
    const removed = before.tokens.filter(token => token.start >= start && token.end <= end);
    announce(added.length ? added.map(token => `Mentioned ${token.title}`).join(". ") : removed.map(token => `Removed ${token.title}`).join(". "));
  };
  const insert = (mention: ComposerMention, query: MentionQuery) => {
    const el = textarea.current;
    if (!el) return;
    const marker = referenceMarkers(mention.insertText).find(marker => marker.kind === "tab");
    const leading = query.start > 0 && !/\s/.test(el.value[query.start - 1]) ? " " : "";
    replace(el, query.start, query.end, `${leading}${mention.insertText} `, marker ? { id: marker.id, title: mention.label } : undefined);
  };
  useLayoutEffect(() => {
    const el = textarea.current;
    if (!el) return;
    const drop = (event: Event) => {
      if (!(event instanceof CustomEvent) || typeof event.detail?.id !== "string") return;
      const tab = mux.getState().tabs[event.detail.id];
      if (!tab) return;
      snapSelection(el);
      insert({ id: tab.id, kind: "tab", label: tab.title, insertText: `<domios type="tab" id="${tab.id}" />`, description: "", group: "Tabs", match: 0 },
        { start: el.selectionStart, end: el.selectionEnd, partial: "" });
    };
    el.addEventListener("domios-insert-tab-mention", drop);
    return () => el.removeEventListener("domios-insert-tab-mention", drop);
  });
  const handleKey = (event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    const el = event.currentTarget;
    const { selectionStart: start, selectionEnd: end, selectionDirection } = el;
    const tokens = model.current!.current.tokens;
    if (event.code === "Backspace" || event.code === "Delete") {
      const range = mentionDeletionRange(tokens, start, end, event.code, event.ctrlKey || event.altKey ? el.value : undefined);
      if (!range) return false;
      event.preventDefault();
      replace(el, range.start, range.end, "");
      return true;
    }
    if ((event.code === "ArrowLeft" || event.code === "ArrowRight") && !event.altKey && !event.ctrlKey && !event.metaKey) {
      const backward = selectionDirection === "backward";
      const focus = event.shiftKey ? backward ? start : end : start === end ? start : null;
      const target = focus === null ? null : mentionArrowTarget(tokens, focus, event.code);
      if (target === null) return false;
      event.preventDefault();
      if (event.shiftKey) {
        const anchor = backward ? end : start;
        el.setSelectionRange(Math.min(anchor, target), Math.max(anchor, target), target < anchor ? "backward" : "forward");
      } else el.setSelectionRange(target, target);
      return true;
    }
    return false;
  };
  const beforeInput = (event: InputEvent<HTMLTextAreaElement>) => {
    if (model.current!.pending) return;
    const type = event.nativeEvent.inputType;
    if (type === "historyUndo" || type === "historyRedo") return;
    const el = event.currentTarget;
    snapSelection(el);
    const direction = type === "deleteContentBackward" || type === "deleteWordBackward" ? "Backspace"
      : type === "deleteContentForward" || type === "deleteWordForward" ? "Delete" : null;
    const range = direction && mentionDeletionRange(model.current!.current.tokens, el.selectionStart, el.selectionEnd, direction, type.includes("Word") ? el.value : undefined);
    if (range && event.cancelable) { event.preventDefault(); replace(el, range.start, range.end, ""); }
  };
  const handlePaste = (event: ClipboardEvent<HTMLTextAreaElement>, fromPortable: (text: string) => string = text => text): boolean => {
    const data = event.clipboardData;
    if (data.files?.length || Array.from(data.items ?? []).some(item => item.kind === "file")) return false;
    const text = normalizePastedMentions(data.getData(DOMIOS_TEXT_MIME) || data.getData("text/plain"));
    if (!referenceMarkers(text).some(marker => marker.kind === "tab")) return false;
    event.preventDefault();
    const el = event.currentTarget;
    const range = snapMentionSelection(model.current!.current.tokens, el.selectionStart, el.selectionEnd);
    const leading = range.start > 0 && !/\s/.test(el.value[range.start - 1]) ? " " : "";
    replace(el, range.start, range.end, `${leading}${fromPortable(text)} `);
    return true;
  };
  const handleCopy = (event: ClipboardEvent<HTMLTextAreaElement>, cut: boolean, toPortable: (text: string) => string = text => text): boolean => {
    const el = event.currentTarget;
    const current = model.current!.current;
    const range = snapMentionSelection(current.tokens, el.selectionStart, el.selectionEnd);
    if (!current.tokens.some(token => token.start < range.end && token.end > range.start)) return false;
    event.preventDefault();
    event.clipboardData.setData("text/plain", toPortable(current.text.slice(range.start, range.end)));
    event.clipboardData.setData(DOMIOS_TEXT_MIME, toPortable(canonicalMentionSlice(current, range.start, range.end)));
    if (cut) replace(el, range.start, range.end, "");
    return true;
  };
  return { draft, announcement, change, insert, handleKey, beforeInput, handlePaste, handleCopy, snapSelection };
}
