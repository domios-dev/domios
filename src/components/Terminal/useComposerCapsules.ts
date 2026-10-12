import { useCallback, useEffect, useMemo, useRef, useState, type ClipboardEvent, type KeyboardEvent, type MutableRefObject, type RefObject } from "react";
import { DOMIOS_TEXT_MIME, type AttachmentKind } from "@codeterm/chat-core";
import { usePersistentDraft } from "../../lib/usePersistentDraft";
import { contextFileName, type ContextCapsule } from "./composerInsertion";
import { useComposerUploads } from "./useComposerUploads";
import { storeBlob } from "./voice/audio";
import {
  absorbAttachmentTags,
  addCapsule,
  capsuleArrowTarget,
  capsuleDeletionRange,
  capsulesForPaths,
  capsuleSpans,
  capsuleToken,
  countLines,
  decodeCapsuleRegistry,
  EMPTY_CAPSULES,
  encodeCapsuleRegistry,
  expandToCapsules,
  hasCapsuleTokens,
  isLongPaste,
  serializeCapsuleDraft,
  type CapsuleRegistry,
  type CapsuleSpan,
} from "./composerCapsules";

export interface InlineAttachment {
  path: string;
  kind?: AttachmentKind;
  label?: string;
  detail?: string;
}

export type InlineAttachmentSink = ((items: InlineAttachment[]) => void) & { upload?: (blob: Blob, name: string) => Promise<string | null | undefined> };

/** Replaces a textarea range through the editing command so native undo keeps the step; falls back where the command is missing. */
export function replaceTextareaRange(el: HTMLTextAreaElement, start: number, end: number, text: string) {
  const expected = el.value.slice(0, start) + text + el.value.slice(end);
  el.focus({ preventScroll: true });
  el.setSelectionRange(start, end);
  try {
    if (typeof document.execCommand === "function") document.execCommand(text ? "insertText" : "delete", false, text);
  } catch {
    // Fall through to the direct edit below.
  }
  if (el.value === expected) return;
  // The prototype setter bypasses React's value tracker so the input event reaches onChange.
  Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")?.set?.call(el, expected);
  el.setSelectionRange(start + text.length, start + text.length);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

/** Pads a capsule insert with spaces only where it would otherwise touch a word. */
function insertWithPadding(el: HTMLTextAreaElement, text: string) {
  const { start, end } = expandToCapsules(el.value, el.selectionStart, el.selectionEnd);
  const before = el.value.slice(0, start);
  const after = el.value.slice(end);
  const lead = before && !/\s$/.test(before) ? " " : "";
  const trail = /^\s/.test(after) ? "" : " ";
  replaceTextareaRange(el, start, end, `${lead}${text}${trail}`);
}

export function useComposerCapsules({ tabId, textareaRef, sinkRef, onError }: {
  tabId: string;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  sinkRef?: MutableRefObject<InlineAttachmentSink | null>;
  onError: (message: string) => void;
}) {
  const [raw, setRaw] = usePersistentDraft(`tab-session-chat-capsules:${tabId}`);
  const [registry, setRegistry] = useState(() => decodeCapsuleRegistry(raw));
  const registryRef = useRef<CapsuleRegistry>(registry);
  registryRef.current = registry;
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;

  const commit = useCallback((next: CapsuleRegistry) => {
    if (next === registryRef.current) return;
    registryRef.current = next;
    setRegistry(next);
    setRaw(encodeCapsuleRegistry(next));
  }, [setRaw]);

  useEffect(() => {
    if (raw !== encodeCapsuleRegistry(registryRef.current)) {
      const next = decodeCapsuleRegistry(raw);
      registryRef.current = next;
      setRegistry(next);
    }
  }, [raw]);

  const uploads = useComposerUploads({
    registry: () => registryRef.current, commit,
    insert: (capsule) => { const el = textareaRef.current; if (el) insertWithPadding(el, capsuleToken(capsule.label)); },
  });
  const clear = () => { uploads.clear(); commit(EMPTY_CAPSULES); };

  const insert = useCallback<InlineAttachmentSink>((items) => {
    const el = textareaRef.current;
    if (!el || items.length === 0) return;
    const added = capsulesForPaths(registryRef.current, items);
    commit(added.registry);
    insertWithPadding(el, added.text);
  }, [commit, textareaRef]);

  const sink = useMemo(() => Object.assign(insert, { upload: uploads.add }), [insert, uploads.add]);

  useEffect(() => {
    if (!sinkRef) return;
    sinkRef.current = sink;
    return () => { if (sinkRef.current === sink) sinkRef.current = null; };
  }, [sink, sinkRef]);

  /** Context capsules land inline at the caret, or at the end when the composer is not focused. */
  const insertContext = useCallback(async (requests: ContextCapsule[]) => {
    const items: InlineAttachment[] = [];
    try {
      for (const request of requests) {
        const path = request.content !== undefined
          ? await storeBlob(new Blob([request.content], { type: "text/plain" }), contextFileName(request))
          : request.path;
        if (path?.trim()) items.push({ path, kind: request.kind, label: request.label, detail: request.detail });
      }
    } catch (error) {
      onErrorRef.current(error instanceof Error ? error.message : String(error));
    }
    const el = textareaRef.current;
    if (!el || items.length === 0) return;
    if (document.activeElement !== el) el.setSelectionRange(el.value.length, el.value.length);
    insert(items);
  }, [insert, textareaRef]);

  const storeLongText = useCallback(async (text: string) => {
    const lines = countLines(text);
    try {
      const path = await storeBlob(new Blob([text], { type: "text/plain" }), `pasted-text-${Date.now()}.txt`);
      const el = textareaRef.current;
      if (!path.trim() || !el) return;
      const added = addCapsule(registryRef.current, { kind: "text", path, lines });
      commit(added.registry);
      insertWithPadding(el, capsuleToken(added.capsule.label));
    } catch (error) {
      onErrorRef.current(error instanceof Error ? error.message : String(error));
    }
  }, [commit, textareaRef]);

  /** True when the paste became capsules; files and images stay with the attachment paste path. */
  const handlePaste = useCallback((event: ClipboardEvent<HTMLTextAreaElement>): boolean => {
    const data = event.clipboardData;
    if (!data || data.files?.length || Array.from(data.items ?? []).some((item) => item.kind === "file")) return false;
    const text = data.getData(DOMIOS_TEXT_MIME) || data.getData("text/plain");
    if (!text) return false;
    const el = event.currentTarget;
    const absorbed = absorbAttachmentTags(text, registryRef.current);
    if (absorbed.text !== text) {
      event.preventDefault();
      commit(absorbed.registry);
      const { start, end } = expandToCapsules(el.value, el.selectionStart, el.selectionEnd);
      replaceTextareaRange(el, start, end, absorbed.text);
      return true;
    }
    if (!isLongPaste(text)) return false;
    event.preventDefault();
    void storeLongText(text);
    return true;
  }, [commit, storeLongText]);

  const handleCopy = useCallback((event: ClipboardEvent<HTMLTextAreaElement>, cut: boolean) => {
    const el = event.currentTarget;
    const { start, end } = expandToCapsules(el.value, el.selectionStart, el.selectionEnd);
    const slice = el.value.slice(start, end);
    if (!hasCapsuleTokens(slice)) return;
    event.preventDefault();
    const portable = serializeCapsuleDraft(slice, registryRef.current);
    event.clipboardData.setData("text/plain", portable);
    event.clipboardData.setData(DOMIOS_TEXT_MIME, portable);
    if (cut) replaceTextareaRange(el, start, end, "");
  }, []);

  /** True when the key acted on a whole capsule. */
  const handleKeyDown = useCallback((event: KeyboardEvent<HTMLTextAreaElement>): boolean => {
    const el = event.currentTarget;
    if (!hasCapsuleTokens(el.value)) return false;
    const { selectionStart: start, selectionEnd: end } = el;
    if (event.code === "Backspace" || event.code === "Delete") {
      const range = capsuleDeletionRange(el.value, start, end, event.code);
      if (!range) return false;
      event.preventDefault();
      replaceTextareaRange(el, range.start, range.end, "");
      return true;
    }
    const plainArrow = !event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey && start === end;
    if (plainArrow && (event.code === "ArrowLeft" || event.code === "ArrowRight")) {
      const target = capsuleArrowTarget(el.value, start, event.code === "ArrowLeft" ? "left" : "right");
      if (target === null) return false;
      event.preventDefault();
      el.setSelectionRange(target, target);
      return true;
    }
    return false;
  }, []);

  const snapSelection = useCallback((el: HTMLTextAreaElement) => {
    if (!hasCapsuleTokens(el.value)) return;
    const { start, end } = expandToCapsules(el.value, el.selectionStart, el.selectionEnd);
    if (start !== el.selectionStart || end !== el.selectionEnd) el.setSelectionRange(start, end, el.selectionDirection ?? "none");
  }, []);

  const remove = useCallback((span: CapsuleSpan) => {
    const el = textareaRef.current;
    if (!el) return;
    const live = capsuleSpans(el.value).find((candidate) => candidate.start === span.start && candidate.label === span.label);
    if (!live) return;
    const before = el.value.slice(0, live.start);
    const spaced = el.value[live.end] === " " && (before === "" || /\s$/.test(before));
    replaceTextareaRange(el, live.start, spaced ? live.end + 1 : live.end, "");
  }, [textareaRef]);

  /** Draft text with capsules as tags: what history and the stash keep, independent of this registry. */
  const toPortable = useCallback((text: string) => serializeCapsuleDraft(text, registryRef.current), []);

  const fromPortable = useCallback((text: string) => {
    const absorbed = absorbAttachmentTags(text, registryRef.current);
    commit(absorbed.registry);
    return absorbed.text;
  }, [commit]);

  return { registry, registryRef, clear, insert, insertContext, handlePaste, handleCopy, handleKeyDown, snapSelection, remove, toPortable, fromPortable, uploads };
}
