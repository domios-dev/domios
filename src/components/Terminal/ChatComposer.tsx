import { AudioLines } from "lucide-react";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { composerAttachmentError } from "./composerAttachmentAdmission";
import { useLatestCallback } from "../../hooks/useLatestCallback";
import { useConfig } from "../../hooks/useConfig";
import { formatAttachmentMarker, type ChatSessionIntent } from "@codeterm/chat-core";
import { canSendDelayedMessage, sendCurrentDelayedMessage } from "./delayedMessageModel";
import { TERMINAL_VIEWER_RECONNECTED_EVENT } from "../../lib/terminalViewerEvents";
import { onResync } from "../../lib/resync";
import { createDelayedMessageFeed } from "./delayedMessageFeed";
import {ProviderCapabilityBadge} from "./ProviderCapabilityBadge";
import { cachedSlashCommands, slashCommandsEqual } from "../../lib/providerCapabilities";
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState, type MutableRefObject, type ReactNode, type RefObject } from "react";
import { ComposerBubbleRow } from "./ComposerBubbles";
import { ComposerDock } from "./ComposerDock";
import { useComposerLayout } from "./useComposerLayout";
import { evaluateWhen } from "../../lib/evaluateWhen";
import { ComposerActions } from "./ComposerActions";
import { isDuplicateSend } from "./chatSendGuard";
import { listen } from "../../lib/bridge";
import { useComposerRecall } from "./useComposerRecall";
import { usePersistentDraft } from "../../lib/usePersistentDraft";
import { useChatDeliveryComposer } from "./chatDeliveryComposer";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { classifyComposerDevice, composerEnterIntent, isComposerEnterKey } from "./composerKeyboard";
import {
  createDelayedMessage,
  deleteDelayedMessage,
  listDelayedMessages,
  listSystemHeartbeats,
  sendDelayedMessageNow,
  updateDelayedMessage,
  type DelayedMessage,
  type RecurrenceRule,
} from "./delayedSend";
import {
  filterSlashCommands,
  type SlashCommandSuggestion,
} from "../../lib/slashCommands";
import type { PendingPrompt } from "../../lib/types";
import { createPortal } from "react-dom";
import { ComposerContextHeader } from "./ComposerContextHeader";
import { DelayedMessageModal, type DelayedModalTarget } from "./DelayedMessageModal";
import { useMuxSelector } from "../../store/multiplexer/hooks";
import { COMPOSER_MIN_HEIGHT, COMPOSER_MAX_HEIGHT } from "./composerAutosize";
import { ComposerExpandHandle, ExpandedComposer } from "./ExpandedComposer";
import {
  beginComposition,
  createCompositionGate,
  endComposition,
  isImeKeyEvent,
} from "./composerComposition";
import { sendTrackedChatMessage, type ChatSendStatus } from "./chatSendStatus";
import { failChatSendRequest, readChatSends, setChatSendDraft } from "./chatSendStore";
import { prepareChatDelivery } from "./chatDeliveryPreparation";
import { composeAttachmentDraft } from "./meshAttachmentHandoff";
import { composerAttachmentHandoff } from "./composerAttachmentHandoff";
import { useComposerInsertions } from "./useComposerInsertions";
import { ComposerCapsuleField } from "./ComposerCapsuleField";
import { ComposerTextInput } from "./ComposerTextInput";
import { useComposerMentionTokens } from "./useComposerMentionTokens";
import { ComposerMentionHighlight } from "./ComposerMentionHighlight";
import { capsuleSpans, capsulesInDraft, hasCapsuleTokens } from "./composerCapsules";
import { replaceTextareaRange, useComposerCapsules, type InlineAttachmentSink } from "./useComposerCapsules";
import { composerSendPlan, resolveComposerSend } from "./composerSendPlan";
import { recordDroppedUploads } from "./DroppedUploadNotice";
import { classifyAttachmentPaths } from "./composerFiles";
import { ComposerMentions, type ComposerMentionsHandle } from "./ComposerMentions";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { useDsTasksExpander } from "./dsTasks/useDsTasksExpander";
import { ReplyQuote } from "@codeterm/chat-ui";
import { inlineReplyText, replyMarker, type ComposerReply } from "./composerReply";
import { useComposerReply } from "./useComposerReply";
import { useStartupSend } from "./useStartupSend";
import { isTranscriptionFailure, TranscriberSetupAction } from "./voice/TranscriberSetupAction";
import { useDictationDraft, useDictationSelection } from "./voice/dictationDraft";
import { useLiveDictation } from "./voice/useLiveDictation";
import { remoteDictationUnavailable } from "../../lib/transport/dictation";
import { ExpandedDictationStatus } from "./voice/DictationStatus";
import { createSpaceDictationHold, SPACE_DICTATION_POP_MS } from "./voice/spaceDictationHold";
import { DICTATION_ACTION } from "../../lib/mentionSources/actions";
import { canonicalMentionSlice, replaceMentionRange } from "./composerMentionTokens";
import type { ComposerMention, MentionQuery } from "../../lib/composerMentions";
import { LIVE_TRANSCRIPT_FILES, useLiveTranscriptHost, type LiveTranscriptFiles } from "./useLiveTranscriptPanel";

const VOICE_TRANSCRIBE_ERROR = "Voice transcription failed";

declare global {
  interface Window {
    __composerPerf?: {
      inputMarks: Array<{ id: number; at: number; inputType: string }>;
      paintLatencies: number[];
      renderCounts: Record<string, number>;
      compositionEvents: string[];
      ignoredKeys: string[];
    };
  }
}


const NO_SUGGESTIONS: SlashCommandSuggestion[] = [];

export function composerHint(hasCommands: boolean, phone: boolean): string {
  if (phone) return hasCommands ? "Ask anything · @ tab · / command" : "Ask anything · @ tab";
  return hasCommands ? "Ask anything · @ to mention a tab · / for commands" : "Ask anything · @ to mention a tab";
}

const SlashPicker = memo(function SlashPicker({
  suggestions,
  highlighted,
  onHighlight,
  onPick,
  triggerRef,
}: {
  suggestions: SlashCommandSuggestion[];
  highlighted: number;
  onHighlight: (index: number) => void;
  onPick: (cmd: SlashCommandSuggestion) => void;
  triggerRef: RefObject<HTMLElement | null>;
}) {
  if (suggestions.length === 0) return null;
  return (
    <AnchoredPopover open anchor={triggerRef} placement="top-start" width="anchor">
    <div role="listbox" aria-label="Command suggestions" className="ct-menu max-h-56 overflow-y-auto">
      {suggestions.map((c, i) => (
        <button
          key={c.name}
          type="button"
          role="option"
          aria-selected={i === highlighted}
          data-testid="slash-command-option"
          data-slash-command={c.name}
          onMouseDown={(e) => e.preventDefault()}
          onMouseEnter={() => onHighlight(i)}
          onClick={() => onPick(c)}
          className="ct-menu-row"
        >
          <span className="shrink-0 font-mono font-medium text-[var(--ct-accent)]">{c.name}</span>
          {c.argumentHint && <span className="shrink-0 font-mono text-[var(--ct-muted-fg)]">{c.argumentHint}</span>}
          <span className="ml-auto min-w-0 truncate text-[var(--ct-muted-fg)]">{c.description}</span>
        </button>
      ))}
    </div>
    </AnchoredPopover>
  );
});

function recordComposerRender(tabId: string): void {
  if (typeof window === "undefined") return;
  const perf = window.__composerPerf;
  if (!perf) return;
  perf.renderCounts[tabId] = (perf.renderCounts[tabId] ?? 0) + 1;
}

let composerInputMarkId = 0;

function recordComposerBeforeInput(inputType: string): void {
  if (typeof window === "undefined") return;
  const perf = window.__composerPerf;
  if (!perf) return;
  perf.inputMarks.push({
    id: composerInputMarkId += 1,
    at: performance.now(),
    inputType,
  });
}

function recordComposerPaint(): void {
  if (typeof window === "undefined") return;
  const perf = window.__composerPerf;
  const mark = perf?.inputMarks.shift();
  if (!perf || !mark) return;
  requestAnimationFrame(() => {
    perf.paintLatencies.push(performance.now() - mark.at);
  });
}

function recordComposerIgnoredKey(key: string): void {
  if (typeof window === "undefined") return;
  window.__composerPerf?.ignoredKeys.push(key);
}

function ChatComposerImpl({
  tabId,
  tabProvider,
  pendingPrompt,
  attachments,
  removeAttachment,
  clearAttachments,
  handlePasteEvent,
  onWillSendMessage,
  addOptimistic,
  updateOptimistic,
  observeChatSendStatus,
  showComposerError,
  promptDock,
  idlePlaceholder,
  replacement,
  inlineAttachmentSink,
  pluginError,
  sessionIntent,
  contextSlot = null,
  isActive = true,
  sendUnavailableReason,
}: {
  tabId: string;
  tabProvider: string | null;
  /** Whether this composer's tab is the currently active/focused tab.
   *  When false, refocusComposer is a no-op — prevents background tabs
   *  from stealing focus after an async send completes. */
  isActive?: boolean;
  sendUnavailableReason?: string | null;
  pendingPrompt: PendingPrompt | null;
  attachments: string[];
  addAttachments: (paths: string[]) => void;
  removeAttachment: (path: string) => void;
  clearAttachments: () => void;
  handlePasteEvent: (event: ClipboardEvent) => void | Promise<void>;
  onWillSendMessage: (text: string) => void;
  addOptimistic: (content: string) => string;
  updateOptimistic: (id: string, content: string) => void;
  observeChatSendStatus: (status: ChatSendStatus) => void;
  showComposerError: (message: string) => void;
  promptDock?: ReactNode;
  /** What the empty input says instead of "Message", e.g. while the agent waits for a sign-in. */
  idlePlaceholder?: string;
  replacement?: ReactNode;
  /** Where pasted and dropped files land as capsules at the caret. */
  inlineAttachmentSink?: MutableRefObject<InlineAttachmentSink | null>;
  pluginError?: string | null;
  sessionIntent?: ChatSessionIntent | null;
  /** Element above the status line that hosts the context header. */
  contextSlot?: HTMLElement | null;
}) {
  recordComposerRender(tabId);
  const transcript = useLiveTranscriptHost();
  const { config } = useConfig(Boolean(transcript));
  const liveTranscriptDefaultMicTap = Boolean((config as { live_transcript?: { default_mic_tap?: boolean } } | null)?.live_transcript?.default_mic_tap);
  const transcriptOwner = transcript?.ownerId === tabId;
  const liveLocked = transcriptOwner && transcript?.locked === true;
  const sendPolicy = useStartupSend(tabId);
  const isActiveRef = useRef(isActive);
  isActiveRef.current = isActive;

  const [persisted, persist] = usePersistentDraft(`tab-session-chat:${tabId}`, showComposerError);
  const [expandedEditor, setExpandedEditor] = useState(false);
  const recall = useComposerRecall(tabId);
  const workspaceId = useMuxSelector(useLatestCallback((state) => state.workspaces.find((workspace) => workspace.tabs.includes(tabId))?.id ?? state.tabs[tabId]?.workspace_id ?? null, [tabId]));
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerHighlight, setPickerHighlight] = useState(0);
  const touchMode = useTouchOptimizedUi();
  const composerDevice = classifyComposerDevice({
    userAgent: navigator.userAgent, platform: navigator.platform,
    maxTouchPoints: navigator.maxTouchPoints, touchOptimizedUi: touchMode,
  });
  const compositionGate = useRef(createCompositionGate()).current;
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const dictationDraft = useDictationDraft(persisted, persist, textareaRef, tabId, compositionGate);
  const { text: input, edit: setInput } = dictationDraft;
  const mentionTokens = useComposerMentionTokens(input, setInput, textareaRef);
  const displayInput = mentionTokens.draft.text;
  useDictationSelection(dictationDraft, mentionTokens.draft);
  const dictationIdentity = useMuxSelector(useLatestCallback(state => {
    const tab = state.tabs[tabId];
    return JSON.stringify([tabId, tab?.pty_id, tab?.agent_session_id, tab?.session_binding_nonce]);
  }, [tabId]));
  const dictationUnavailable = remoteDictationUnavailable(tabId);
  const dictation = useLiveDictation(tabId, dictationIdentity, isActive && !replacement && !liveLocked && !dictationUnavailable, {
    prepare(session) {
      const editor = textareaRef.current;
      if (!editor || editor.value !== displayInput) return false;
      const { selectionStart: start, selectionEnd: end } = editor;
      const protectedRanges = [...mentionTokens.draft.tokens, ...capsuleSpans(displayInput)]
        .filter(span => start < span.end && end > span.start);
      const anchor = protectedRanges.length ? Math.max(end, ...protectedRanges.map(span => span.end)) : start;
      dictationDraft.start(session, mentionTokens.draft, anchor, Math.max(anchor, end));
      return true;
    },
    update: update => dictationDraft.update(update, mentionTokens.draft),
    cancel: () => dictationDraft.cancel(mentionTokens.draft),
    composing: () => compositionGate.active,
  });
  const dictationRef = useRef(dictation);
  dictationRef.current = dictation;
  const [spaceArming, setSpaceArming] = useState(false);
  const [spacePop, setSpacePop] = useState(false);
  const spacePopTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const spaceHold = useRef<ReturnType<typeof createSpaceDictationHold> | null>(null);
  if (!spaceHold.current) {
    spaceHold.current = createSpaceDictationHold({
      onPhaseChange: (phase) => setSpaceArming(phase === "arming"),
      onStartDictation: () => { void dictationRef.current.start(); },
      onStopDictation: () => { void dictationRef.current.stop(); },
      onInsertSpace: () => {
        const editor = textareaRef.current;
        if (editor && document.activeElement === editor) {
          replaceTextareaRange(editor, editor.selectionStart, editor.selectionEnd, " ");
        }
      },
      onPop: () => {
        if (spacePopTimerRef.current !== null) clearTimeout(spacePopTimerRef.current);
        setSpacePop(true);
        spacePopTimerRef.current = setTimeout(() => {
          spacePopTimerRef.current = null;
          setSpacePop(false);
        }, SPACE_DICTATION_POP_MS);
      },
    });
  }
  useEffect(() => {
    if (dictation.phase === "idle") spaceHold.current?.resetIfDictationEnded();
  }, [dictation.phase]);
  useEffect(() => {
    if (!isActive || replacement || liveLocked) {
      spaceHold.current?.dispose();
      if (spacePopTimerRef.current !== null) { clearTimeout(spacePopTimerRef.current); spacePopTimerRef.current = null; }
      setSpacePop(false);
      return;
    }
    const onKeyUp = (event: KeyboardEvent) => {
      spaceHold.current?.onKeyUp(event.code, document.activeElement === textareaRef.current);
    };
    const onBlur = () => spaceHold.current?.onBlur();
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onBlur);
      spaceHold.current?.dispose();
      if (spacePopTimerRef.current !== null) { clearTimeout(spacePopTimerRef.current); spacePopTimerRef.current = null; }
    };
  }, [isActive, replacement, liveLocked]);
  const shellRef = useRef<HTMLDivElement>(null);
  const { layout: composerLayout, measure: measureComposer } = useComposerLayout(input, textareaRef, shellRef, compositionGate, !replacement, expandedEditor);
  const lastSendRef = useRef<{ text: string; at: number }>({ text: "", at: 0 });
  const pendingDraftsRef = useRef(new Set<string>());
  const [delayedMessages, setDelayedMessages] = useState<DelayedMessage[]>([]);
  const [systemHeartbeats, setSystemHeartbeats] = useState<DelayedMessage[]>([]);
  const [delayedBusyId, setDelayedBusyId] = useState<string | null>(null);
  const [delayedModal, setDelayedModal] = useState<DelayedModalTarget | null>(null);
  const [scheduleBusy, setScheduleBusy] = useState(false);
  const [transcriptionFailure, setTranscriptionFailure] = useState<string | null>(null);
  // Screen-reader-visible mirror of showComposerError: the field's current
  // content was rejected, until the user edits it again.
  const [composerInvalid, setComposerInvalid] = useState(false);
  const flagComposerError = useLatestCallback(
    (message: string) => {
      setComposerInvalid(true);
      showComposerError(message);
    },
    [showComposerError],
  );
  const provider = useAgentDescriptor(tabProvider);
  const keepReplyInline = Boolean(tabProvider && provider?.attachments && !provider.attachments.filePaths && provider.attachments.inlineText);
  const capsules = useComposerCapsules({ tabId, textareaRef, sinkRef: inlineAttachmentSink, onError: flagComposerError,
    keepTextInline: Boolean(tabProvider && !provider?.attachments?.filePaths), focusAttachments: !transcriptOwner });
  const addLiveFiles = useLatestCallback((event: Event) => {
    const detail = (event as CustomEvent<LiveTranscriptFiles>).detail;
    if (detail?.ownerId !== tabId || !textareaRef.current) return;
    if ("blob" in detail) {
      detail.upload ??= capsules.uploads.add(detail.blob, detail.name);
    } else if (!detail.accepted) {
      detail.accepted = true;
      capsules.insert(detail.items);
    }
  }, [tabId]);
  useEffect(() => {
    if (!transcriptOwner || replacement) return;
    window.addEventListener(LIVE_TRANSCRIPT_FILES, addLiveFiles);
    return () => window.removeEventListener(LIVE_TRANSCRIPT_FILES, addLiveFiles);
  }, [addLiveFiles, replacement, transcriptOwner]);
  useChatDeliveryComposer(tabId, text => {
    const existing = input;
    setInput(capsules.fromPortable([existing, text].filter(Boolean).join("\n")));
    textareaRef.current?.focus({ preventScroll: true });
  });
  const [composing, setComposing] = useState(false);
  const capsulesActive = !composing && hasCapsuleTokens(input);
  const attachInline = async (paths: string[]) => capsules.insert(await classifyAttachmentPaths(paths));
  const commands = useMuxSelector(useLatestCallback(state => state.tabs[tabId]?.agent_runtime_state?.controls?.commands !== undefined
    ? (state.tabs[tabId]?.agent_runtime_state?.controls?.commands ?? []).map(command => ({name:`/${command.name}`,description:command.description,argumentHint:command.argumentHint}))
    : cachedSlashCommands(state.tabs[tabId]?.provider_signals?.capabilities ?? (state.tabs[tabId]?.backing?.kind === "chat_backend" ? {status:"unavailable",reason:"Commands not declared"} : undefined)), [tabId]), slashCommandsEqual);
  const firstLine = input.split("\n")[0] || "";
  const suggestions = useMemo(
    () => filterSlashCommands(firstLine, commands),
    [firstLine, commands],
  );

  const refocusComposer = useLatestCallback(() => {
    if (!isActiveRef.current) return;
    const focus = () => {
      if (!isActiveRef.current) return;
      textareaRef.current?.focus({ preventScroll: true });
    };
    if (!textareaRef.current) return;
    focus();
    requestAnimationFrame(() => {
      if (document.activeElement !== textareaRef.current) {
        focus();
      }
    });
  }, []);

  useComposerInsertions(tabId, capsules.insertContext);
  const reply = useComposerReply(tabId, flagComposerError, refocusComposer, keepReplyInline);
  const mentionPicker = useRef<ComposerMentionsHandle>(null);
  function pickMention(mention: ComposerMention, query: MentionQuery) {
    if (mention.id !== DICTATION_ACTION) { mentionTokens.insert(mention, query); return; }
    if (dictationUnavailable) { flagComposerError(dictationUnavailable); return; }
    const editor = textareaRef.current;
    if (!editor || editor.value !== displayInput || dictation.active()) return;
    const draft = mentionTokens.draft;
    const anchor = canonicalMentionSlice(draft, 0, query.start).length;
    const canonical = replaceMentionRange(draft, query.start, query.end, "");
    replaceTextareaRange(editor, query.start, query.end, "", false);
    void dictation.start(session => {
      dictationDraft.startAt(session, canonical, anchor, anchor);
      return true;
    });
  }
  const { expand: expandDsTasks, plansSink } = useDsTasksExpander(tabId);

  useLayoutEffect(() => { recordComposerPaint(); }, [input]);

  useEffect(() => {
    setPickerOpen(firstLine.startsWith("/") && suggestions.length > 0);
    setPickerHighlight(0);
  }, [firstLine, suggestions.length]);

  useEffect(() => {
    const id = setTimeout(() => {
      if (isActiveRef.current) textareaRef.current?.focus();
    }, 0);
    return () => clearTimeout(id);
  }, []);

  const delayedFeedRef = useRef<ReturnType<typeof createDelayedMessageFeed> | null>(null);
  const delayedTabRef = useRef<string | null>(null);
  const refreshDelayedMessages = useLatestCallback(async () => {
    await delayedFeedRef.current?.refresh();
  }, []);

  useEffect(() => {
    if (!isActive) return;
    if (delayedTabRef.current !== tabId) {
      setDelayedMessages([]);
      setSystemHeartbeats([]);
    }
    delayedTabRef.current = tabId;
    const feed = createDelayedMessageFeed(tabId, {
      load: listDelayedMessages,
      publish: (messages) => {
        setDelayedMessages(messages);
        setDelayedModal((target) => target?.kind === "edit"
          ? (messages.some((message) => message.id === target.message.id && canSendDelayedMessage(message)) ? target : null)
          : target);
      },
      subscribe: (changed) => listen<{ tabId?: string }>("delayed-send-changed", (event) => {
        changed(event.payload?.tabId);
      }),
    });
    const heartbeatFeed = createDelayedMessageFeed(tabId, {
      load: listSystemHeartbeats,
      publish: setSystemHeartbeats,
      subscribe: changed => listen<{ tabId?: string }>("delayed-send-changed", event => changed(event.payload?.tabId)),
    });
    const refresh = () => { void feed.wake(); void heartbeatFeed.wake(); };
    const wake = () => {
      if (document.visibilityState !== "hidden") refresh();
    };
    const stopResync = onResync(refresh);
    document.addEventListener("visibilitychange", wake);
    window.addEventListener("focus", wake);
    window.addEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, wake);
    delayedFeedRef.current = feed;
    return () => {
      stopResync();
      document.removeEventListener("visibilitychange", wake);
      window.removeEventListener("focus", wake);
      window.removeEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, wake);
      feed.dispose();
      heartbeatFeed.dispose();
      if (delayedFeedRef.current === feed) delayedFeedRef.current = null;
    };
  }, [tabId, isActive]);

  function pickSuggestion(cmd: SlashCommandSuggestion) {
    const lines = input.split("\n");
    lines[0] = cmd.name + " ";
    setInput(lines.join("\n"));
    setPickerOpen(false);
    refocusComposer();
  }

  function buildDraft(extraAttachments: string[] = [], canonical = input) {
    const text = canonical.trim();
    const cleanExtraAttachments = extraAttachments
      .map((p) => p.trim())
      .filter((p, i, all) => p.length > 0 && all.indexOf(p) === i);
    const allAttachments = [...attachments];
    for (const p of cleanExtraAttachments) {
      if (!allAttachments.includes(p)) allAttachments.push(p);
    }
    const quoted = reply.replyRef.current;
    if (!text && allAttachments.length === 0 && !quoted) return;
    return { text, allAttachments, recordedVoiceAttachments: cleanExtraAttachments, quoted };
  }

  async function composeDraft(
    text: string,
    allAttachments: string[],
    recordedVoiceAttachments: string[],
    quoted: ComposerReply | null,
  ) {
    return composeAttachmentDraft(
      tabId,
      text,
      allAttachments,
      recordedVoiceAttachments,
      composerAttachmentHandoff,
      capsules.registryRef.current,
      quoted,
      keepReplyInline,
    );
  }

  async function send(extraAttachments: string[] = [], canonical?: string) {
    if (!sendPolicy.allowed || sendUnavailableReason) return;
    const draft = buildDraft(extraAttachments, canonical);
    if (!draft) return;
    const { text, allAttachments, recordedVoiceAttachments, quoted } = draft;
    const target = { expectedRecipient: sendPolicy.recipient };
    const attachmentError = composerAttachmentError(provider?.attachments, text, allAttachments,
      recordedVoiceAttachments, capsules.registryRef.current, quoted);
    if (tabProvider && attachmentError) { flagComposerError(attachmentError); return; }
    const registry = capsules.registryRef.current;
    const dupKey = JSON.stringify([text, allAttachments, recordedVoiceAttachments,
      capsulesInDraft(text, registry).map(({ kind, path, uploadId }) => [kind, path, uploadId]), quoted]);
    const duplicate = pendingDraftsRef.current.has(dupKey)
      || isDuplicateSend(lastSendRef.current, dupKey, Date.now());
    const portable = capsules.toPortable(text);
    setExpandedEditor(false);
    setInput("");
    if (textareaRef.current) textareaRef.current.value = "";
    clearAttachments();
    const plan = composerSendPlan(text, registry, capsules.uploads.uploads);
    const capturedUploads = duplicate ? [] : capsules.uploads.take(plan.uploadIds);
    capsules.detach();
    reply.clearIfSent(quoted);
    if (duplicate) return;
    lastSendRef.current = { text: dupKey, at: Date.now() };
    pendingDraftsRef.current.add(dupKey);
    onWillSendMessage(text);
    const voicePendingContent = recordedVoiceAttachments.map(path => formatAttachmentMarker(path, "voice", "voice")).join("\n");
    const replyPreview = quoted
      ? (keepReplyInline || !quoted.path ? inlineReplyText(quoted) : replyMarker(quoted))
      : "";
    const optimisticId = addOptimistic(recordedVoiceAttachments.length > 0
      ? voicePendingContent : portable || allAttachments.join(" ") || replyPreview);
    const deliveryDraft = { text, attachments: allAttachments, recordedVoiceAttachments,
      ...(recordedVoiceAttachments.length ? { voiceTranscribing: true } : {}),
      capsules: registry, reply: quoted, ...(keepReplyInline ? { inlineReply: true } : {}) };
    setChatSendDraft(tabId, optimisticId, deliveryDraft, target);
    setComposerInvalid(false);
    refocusComposer();
    let composed: string;
    try {
      const send = readChatSends(tabId).find((item) => item.id === optimisticId)
        ?? { id: optimisticId, correlationId: optimisticId, content: portable, sentAtMs: Date.now(), draft: deliveryDraft };
      const results = await Promise.all(capturedUploads.map(async item => ({ ...item, path: await item.result })));
      const resolved = resolveComposerSend(text, registry, new Map(results.map(item => [item.upload.id, item.path])));
      recordDroppedUploads(tabId, optimisticId, results.filter(item => !item.path).map(item => item.upload));
      if (!resolved.text.trim() && !allAttachments.length && !quoted) {
        const dropped = readChatSends(tabId).find(item => item.id === optimisticId)?.droppedUploads;
        throw new Error(dropped?.length ? `No message was sent because its attachments failed: ${dropped.join(", ")}`
          : "No message was sent because the prepared message is empty.");
      }
      const readyDraft = { ...deliveryDraft, ...resolved, voiceTranscribing: false };
      setChatSendDraft(tabId, optimisticId, readyDraft);
      composed = await prepareChatDelivery(tabId, { ...send, draft: readyDraft }, expandDsTasks);
      if (!composed.trim()) {
        failChatSendRequest(tabId, optimisticId, "No message was sent because the prepared message is empty.");
        pendingDraftsRef.current.delete(dupKey);
        return;
      }
      updateOptimistic(optimisticId, composed);
    } catch (error) {
      if (recordedVoiceAttachments.length) setChatSendDraft(tabId, optimisticId, { ...deliveryDraft, voiceTranscribing: false });
      failChatSendRequest(tabId, optimisticId, error);
      pendingDraftsRef.current.delete(dupKey);
      return;
    }
    try {
      const status = await sendTrackedChatMessage(tabId, optimisticId, composed, target);
      observeChatSendStatus(status);
      recall.remember(portable);
    } catch (e) {
      console.warn("[chat-send] optimistic delivery failed", {
        tabId, messageId: optimisticId, error: e instanceof Error ? e.message : String(e),
      });
    } finally {
      pendingDraftsRef.current.delete(dupKey);
      refocusComposer();
    }
  }

  async function scheduleDraft(text: string, scheduledAtMs: number | null, recurrence?: RecurrenceRule) {
    const draft = buildDraft() ?? { allAttachments: [], recordedVoiceAttachments: [], quoted: null };
    const body = text.trim();
    if (!body && draft.allAttachments.length === 0 && !draft.quoted) return;
    const attachmentError = composerAttachmentError(provider?.attachments, body, draft.allAttachments,
      draft.recordedVoiceAttachments, capsules.registryRef.current, draft.quoted);
    if (tabProvider && attachmentError) { flagComposerError(attachmentError); return; }
    setScheduleBusy(true);
    setTranscriptionFailure(null);
    try {
      const pendingDraft = composeDraft(
        body,
        draft.allAttachments,
        draft.recordedVoiceAttachments,
        draft.quoted,
      );
      const { composed, transcribeFailed } = await pendingDraft;
      if (transcribeFailed) {
        setDelayedModal(null);
        setTranscriptionFailure(VOICE_TRANSCRIBE_ERROR);
        return;
      }

      await createDelayedMessage(tabId, composed, scheduledAtMs, recurrence);
      setInput("");
      clearAttachments();
      capsules.clear();
      reply.clearIfSent(draft.quoted);
      setDelayedModal(null);
      await refreshDelayedMessages();
    } catch (e) {
      const reason = e instanceof Error ? e.message : String(e);
      if (isTranscriptionFailure(reason)) {
        setDelayedModal(null);
        setTranscriptionFailure(reason);
      }
      else flagComposerError(reason);
    } finally {
      setScheduleBusy(false);
      refocusComposer();
    }
  }

  async function withDelayedBusy(id: string, op: () => Promise<void>) {
    setDelayedBusyId(id);
    try {
      await op();
      await refreshDelayedMessages();
    } catch (e) {
      showComposerError(e instanceof Error ? e.message : String(e));
      await refreshDelayedMessages();
    } finally {
      setDelayedBusyId(null);
      refocusComposer();
    }
  }

  function closeDelayedModal() {
    setDelayedModal(null);
    refocusComposer();
  }

  function runOnDelayed(id: string, op: () => Promise<unknown>) {
    setDelayedModal(null);
    void withDelayedBusy(id, async () => {
      await op();
    });
  }

  // Stable identities so the memoized header and pickers sit out keystrokes.
  const openScheduleModal = useLatestCallback(() => setDelayedModal({ kind: "create", text: input.trim() }), []);
  const editDelayed = useLatestCallback((message: DelayedMessage) => setDelayedModal({ kind: "edit", message }), []);
  const deleteDelayed = useLatestCallback((id: string) => void withDelayedBusy(id, async () => {
    await deleteDelayedMessage(id);
  }), []);
  const pickSlashCommand = useLatestCallback(pickSuggestion, []);
  const sendFinalized = useLatestCallback(() => {
    if (isActiveRef.current) void send([], dictationDraft.current());
  }, []);
  function sendFromComposer() {
    if (!sendPolicy.allowed || sendUnavailableReason) return;
    if (dictation.active()) dictation.finishThen(sendFinalized);
    else void send([], dictationDraft.current());
  }

  const openLiveTranscript = useLatestCallback(() => {
    if (!transcript || liveLocked) return;
    if (dictation.active()) {
      void Promise.resolve(dictation.cancel()).finally(() => {
        transcript.open(tabId, { autostart: true });
      });
      return;
    }
    transcript.open(tabId, { autostart: true });
  }, [liveLocked, tabId, transcript]);

  if (replacement) return <ComposerDock mode={composerLayout.mode} ref={shellRef}>{replacement}</ComposerDock>;

  const composerActions = (
    <ComposerActions
      expanded={expandedEditor}
      tabId={tabId}
      onAttach={(paths) => void attachInline(paths)}
      uploadFiles={capsules.uploads.addFiles}
      onSend={sendFromComposer}
      onSchedule={!liveLocked && dictation.phase === "idle" ? () => { setExpandedEditor(false); openScheduleModal(); } : undefined}
      onSendWithAttachments={(paths) => void send(paths)}
      onError={flagComposerError}
      sendDisabled={liveLocked || !sendPolicy.allowed || (!input.trim() && attachments.length === 0 && !reply.reply)}
      sendUnavailableReason={sendUnavailableReason}
      dictation={liveLocked ? undefined : dictation}
      onOpenLiveTranscript={transcript && !liveLocked ? openLiveTranscript : undefined}
      liveTranscriptOnTap={liveTranscriptDefaultMicTap && !liveLocked && Boolean(transcript?.canOpen(tabId))}
      hideActionButton={liveLocked}
      inputOverlay={liveLocked && transcript ? (
        <div className="ct-live-lock" data-testid="live-transcript-composer-lock">
          <span className="ct-live-lock-label">
            <span className="ct-live-dot" data-live="true" aria-hidden="true" />
            <span className="ct-live-lock-title">Live Transcript is listening</span>
            <span className="ct-live-lock-hint hidden sm:inline">Drop or paste files here</span>
          </span>
          <button type="button" className="ct-live-lock-open" onClick={() => transcript.open(tabId)}>
            <AudioLines size={13} aria-hidden="true" />Open Transcript
          </button>
        </div>
      ) : undefined}
      spaceArming={spaceArming}
      spacePop={spacePop}
      hasText={liveLocked || input.trim().length > 0 || attachments.length > 0 || reply.reply !== null}
      cardHeader={(reply.reply || attachments.length > 0) && <>
        {reply.reply && <ReplyQuote label={reply.reply.label} excerpt={reply.reply.excerpt} onRemove={() => { reply.clear(); refocusComposer(); }} />}
        {attachments.length > 0 && <ComposerBubbleRow attachments={attachments} onRemove={removeAttachment} />}
      </>}
    >
      {transcriptionFailure && <TranscriberSetupAction reason={transcriptionFailure} />}
      <SlashPicker
        suggestions={!liveLocked && pickerOpen ? suggestions : NO_SUGGESTIONS}
        highlighted={pickerHighlight}
        triggerRef={textareaRef}
        onHighlight={setPickerHighlight}
        onPick={pickSlashCommand}
      />
      {!liveLocked && <ComposerMentions ref={mentionPicker} tabId={tabId} input={displayInput} setInput={mentionTokens.change} onInsert={pickMention} tokens={mentionTokens.draft.tokens}
        onPicked={refocusComposer} textareaRef={textareaRef} anchorRef={shellRef} />}
      {plansSink}
      {!sendPolicy.allowed && <span role="status">Connecting to chat — sending is unavailable</span>}
      <ComposerCapsuleField
        tabId={tabId}
        value={displayInput}
        registry={capsules.registry}
        active={capsulesActive}
        textareaRef={textareaRef}
        marginPx={composerLayout.editorPx > COMPOSER_MIN_HEIGHT ? 8 : 0}
        onRemove={capsules.remove}
        uploads={capsules.uploads.uploads}
        onRetry={(id) => { void capsules.uploads.retry(id); }}
      >
        <span role="status" aria-live="polite" className="sr-only">{mentionTokens.announcement}</span>
        <ComposerTextInput
          data-testid="chat-composer"
          data-tab-id={tabId}
          data-dictating={dictation.phase === "listening" ? "true" : undefined}
          aria-label="Message composer"
          readOnly={liveLocked}
          aria-invalid={composerInvalid}
          ref={textareaRef}
          value={displayInput}
          onChange={(e) => {
            setComposerInvalid(false);
            recall.reset();
            mentionTokens.change(e.target.value, (e.nativeEvent as InputEvent).inputType);
            mentionPicker.current?.syncSelection(e.currentTarget);
          }}
          onBeforeInput={(e) => {
            recordComposerBeforeInput(e.nativeEvent.inputType || "beforeinput");
            mentionTokens.beforeInput(e);
            if (!e.defaultPrevented) dictationDraft.noteSelection(mentionTokens.draft, e.nativeEvent.inputType || "");
          }}
          onKeyDown={(e) => {
            if (liveLocked) return;
            recordComposerIgnoredKey(e.key);
            if (isImeKeyEvent(compositionGate, e.nativeEvent.isComposing, e.nativeEvent.keyCode)) return;
            if (e.code === "Escape" && dictation.active()) {
              e.preventDefault();
              e.stopPropagation();
              dictation.cancel();
              return;
            }
            if (!dictationUnavailable && spaceHold.current?.onKeyDown({
              code: e.code,
              repeat: e.repeat,
              shiftKey: e.shiftKey,
              ctrlKey: e.ctrlKey,
              metaKey: e.metaKey,
              altKey: e.altKey,
              isComposing: e.nativeEvent.isComposing,
            }, {
              empty: input.length === 0 && attachments.length === 0 && !reply.reply,
              dictating: dictation.active(),
              pickerOpen,
              composing: compositionGate.active,
            })) {
              e.preventDefault();
              return;
            }
            let enterIntent = composerEnterIntent(composerDevice, {
              code: e.code, key: e.key, keyCode: e.nativeEvent.keyCode, shiftKey: e.shiftKey, ctrlKey: e.ctrlKey,
              metaKey: e.metaKey, altKey: e.altKey, isComposing: e.nativeEvent.isComposing,
            });
            if (mentionPicker.current?.handleKey(e)) return;
            if (mentionTokens.handleKey(e)) { mentionPicker.current?.syncSelection(e.currentTarget); return; }
            if (expandedEditor && isComposerEnterKey(e)) {
              enterIntent = (e.ctrlKey || e.metaKey) ? "submit" : "newline";
            }
            if (isComposerEnterKey(e)) {
              if (enterIntent !== "submit") {
                if (enterIntent === "newline") setPickerOpen(false);
                return;
              }
            }
            if (capsules.handleKeyDown(e)) return;
            const shortcutContext = { composerFocus: isActive, composing: compositionGate.active,
              pickerOpen, atStart: e.currentTarget.selectionStart === 0 };
            if ((e.code === "ArrowUp" || e.code === "ArrowDown") && evaluateWhen("composerFocus && !composing && !pickerOpen && atStart", shortcutContext)) {
              const previous = recall.move(capsules.toPortable(input), e.code === "ArrowUp" ? -1 : 1);
              if (previous !== undefined) { e.preventDefault(); setInput(capsules.fromPortable(previous)); }
              return;
            }
            if (e.code === "KeyS" && (e.ctrlKey || e.metaKey) && e.shiftKey && evaluateWhen("composerFocus && !composing && !pickerOpen", shortcutContext)) {
              e.preventDefault();
              setInput(capsules.fromPortable(recall.stash(capsules.toPortable(input))));
              return;
            }
            if (pickerOpen) {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setPickerHighlight((h) => Math.min(h + 1, suggestions.length - 1));
                return;
              }
              if (e.key === "ArrowUp") {
                e.preventDefault();
                setPickerHighlight((h) => Math.max(h - 1, 0));
                return;
              }
              if (e.key === "Tab" || enterIntent === "submit") {
                e.preventDefault();
                const cmd = suggestions[pickerHighlight];
                if (cmd) pickSuggestion(cmd);
                return;
              }
              if (e.key === "Escape") {
                e.preventDefault();
                setPickerOpen(false);
                return;
              }
            }
            if (e.key === "Escape" && !input && reply.reply) {
              e.preventDefault();
              e.stopPropagation();
              reply.clear();
              return;
            }
            if (enterIntent === "submit") {
              e.preventDefault();
              sendFromComposer();
            }
          }}
          onCompositionStart={() => {
            if (textareaRef.current) mentionTokens.snapSelection(textareaRef.current);
            dictationDraft.noteSelection(mentionTokens.draft, "insertCompositionText");
            beginComposition(compositionGate);
            if (capsulesActive) setComposing(true);
            if (typeof window !== "undefined") window.__composerPerf?.compositionEvents.push("start");
          }}
          onCompositionEnd={(e) => {
            endComposition(compositionGate);
            setComposing(false);
            // Commit the finished draft once and run the autosize the gate
            // deferred; both are no-ops when state already mirrors the DOM.
            mentionTokens.change(e.currentTarget.value);
            dictation.compositionEnded();
            measureComposer();
            if (typeof window !== "undefined") window.__composerPerf?.compositionEvents.push("end");
          }}
          onPaste={(e) => {
            if (liveLocked) {
              e.preventDefault();
              void handlePasteEvent(e.nativeEvent);
              return;
            }
            if (mentionTokens.handlePaste(e, capsules.fromPortable)) return;
            if (capsules.handlePaste(e)) return;
            const data = e.clipboardData;
            const hasFiles = data.files?.length || Array.from(data.items ?? []).some((item) => item.kind === "file");
            if (hasFiles) {
              const text = data.getData("text/plain");
              e.preventDefault();
              if (text) replaceTextareaRange(e.currentTarget, e.currentTarget.selectionStart, e.currentTarget.selectionEnd, text);
            }
            void handlePasteEvent(e.nativeEvent);
          }}
          onCopy={(e) => { capsules.snapSelection(e.currentTarget); if (!mentionTokens.handleCopy(e, false, capsules.toPortable)) capsules.handleCopy(e, false); }}
          onCut={(e) => { if (liveLocked) { e.preventDefault(); return; } capsules.snapSelection(e.currentTarget); if (!mentionTokens.handleCopy(e, true, capsules.toPortable)) capsules.handleCopy(e, true); }}
          onSelect={(e) => { capsules.snapSelection(e.currentTarget); mentionTokens.snapSelection(e.currentTarget); mentionPicker.current?.syncSelection(e.currentTarget); }}
          placeholder={pendingPrompt
            ? "Answer the request above to continue."
            : idlePlaceholder ?? composerHint(commands.length > 0, composerDevice === "phone")}
          enterKeyHint={composerDevice === "phone" ? "enter" : "send"}
          style={{
            color: capsulesActive ? "transparent" : "var(--ct-fg, #c9d1d9)",
            lineHeight: `${COMPOSER_MIN_HEIGHT}px`,
            // Margin, not padding: padding would feed back into the scrollHeight autosize.
            margin: composerLayout.editorPx > COMPOSER_MIN_HEIGHT ? "8px 0" : 0,
          }}
        />
        <ComposerMentionHighlight draft={mentionTokens.draft} textarea={textareaRef} marginPx={composerLayout.editorPx > COMPOSER_MIN_HEIGHT ? 8 : 0} />
      </ComposerCapsuleField>
    </ComposerActions>
  );
  return (
    <ComposerDock mode={composerLayout.mode} ref={shellRef}>
      {contextSlot && createPortal(
        <ComposerContextHeader
          isActive={isActive}
          pluginError={pluginError}
          sessionIntent={sessionIntent}
          tabId={tabId}
          workspaceId={workspaceId}
          delayed={delayedMessages}
          heartbeats={systemHeartbeats}
          onEditDelayed={editDelayed}
          onDeleteDelayed={deleteDelayed}
          onScheduleDelayed={openScheduleModal}
          draftActive={input.length > 0}
        />,
        contextSlot,
      )}
      {delayedModal?.kind === "create" && (
        <DelayedMessageModal
          target={delayedModal}
          busy={scheduleBusy}
          onClose={closeDelayedModal}
          onStash={(text) => void scheduleDraft(text, null)}
          onSubmit={(patch) => void scheduleDraft(patch.text, patch.scheduledAtMs, patch.recurrence)}
        />
      )}
      {delayedModal?.kind === "edit" && (() => {
        const id = delayedModal.message.id;
        return (
          <DelayedMessageModal
            key={id}
            target={delayedModal}
            busy={delayedBusyId === id}
            onClose={closeDelayedModal}
            onDelete={() => runOnDelayed(id, () => deleteDelayedMessage(id))}
            onSendNow={() => runOnDelayed(id, () => sendCurrentDelayedMessage(id, { load: () => listDelayedMessages(tabId), send: sendDelayedMessageNow }))}
            onSubmit={(patch) => runOnDelayed(id, () => updateDelayedMessage(id, patch))}
          />
        );
      })()}
      <div className="mb-2 flex flex-wrap items-center gap-1 pl-8 empty:hidden">
      <ProviderCapabilityBadge tabId={tabId} />
      </div>
      {promptDock}
      <div data-testid="composer-row" className="relative">
        {(composerLayout.scrolls || composerLayout.editorPx >= COMPOSER_MAX_HEIGHT) &&
          <ComposerExpandHandle onExpand={() => { setPickerOpen(false); setExpandedEditor(true); }} />}

        {expandedEditor && isActive
          ? <ExpandedComposer
              anchorRef={shellRef}
              onClose={() => setExpandedEditor(false)}
              headerStatus={dictation.phase !== "idle" ? <ExpandedDictationStatus phase={dictation.phase} /> : undefined}
            >
              {composerActions}
            </ExpandedComposer>
          : composerActions}
      </div>
    </ComposerDock>
  );
}

export const ChatComposer = memo(ChatComposerImpl, (a, b) =>
  a.tabId === b.tabId
  && a.tabProvider === b.tabProvider
  && a.pendingPrompt === b.pendingPrompt
  && a.attachments === b.attachments
  && a.addAttachments === b.addAttachments
  && a.removeAttachment === b.removeAttachment
  && a.clearAttachments === b.clearAttachments
  && a.handlePasteEvent === b.handlePasteEvent
  && a.onWillSendMessage === b.onWillSendMessage
  && a.addOptimistic === b.addOptimistic
  && a.updateOptimistic === b.updateOptimistic
  && a.observeChatSendStatus === b.observeChatSendStatus
  && a.showComposerError === b.showComposerError
  && a.replacement === b.replacement
  && a.promptDock === b.promptDock
  && a.idlePlaceholder === b.idlePlaceholder
  && a.inlineAttachmentSink === b.inlineAttachmentSink
  && a.pluginError === b.pluginError
  && a.sessionIntent === b.sessionIntent
  && a.contextSlot === b.contextSlot
  && a.isActive === b.isActive
  && a.sendUnavailableReason === b.sendUnavailableReason
);
