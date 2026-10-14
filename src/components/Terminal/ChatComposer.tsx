import { canSendDelayedMessage, sendCurrentDelayedMessage } from "./delayedMessageModel";
import { TERMINAL_VIEWER_RECONNECTED_EVENT } from "../../lib/terminalViewerEvents";
import { onResync } from "../../lib/resync";
import { createDelayedMessageFeed } from "./delayedMessageFeed";
import {ProviderCapabilityBadge} from "./ProviderCapabilityBadge";
import {cachedSlashCommands} from "../../lib/providerCapabilities";
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type MutableRefObject, type ReactNode } from "react";
import { ComposerBubbleRow } from "./ComposerBubbles";
import { ComposerDock } from "./ComposerDock";
import { useComposerLayout } from "./useComposerLayout";
import { evaluateWhen } from "../../lib/evaluateWhen";
import { ComposerActions } from "./ComposerActions";
import { isDuplicateSend } from "./chatSendGuard";
import { invoke, listen } from "../../lib/bridge";
import { ComposerContextMeter } from "./ComposerContextMeter";
import { useComposerRecall } from "./useComposerRecall";
import { usePersistentDraft } from "../../lib/usePersistentDraft";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { classifyComposerDevice, composerEnterIntent, isComposerEnterKey } from "./composerKeyboard";
import {
  createDelayedMessage,
  deleteDelayedMessage,
  listDelayedMessages,
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
import { useMuxSelector } from "../../store/multiplexer";
import { COMPOSER_MIN_HEIGHT } from "./composerAutosize";
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
import { hasCapsuleTokens } from "./composerCapsules";
import { replaceTextareaRange, useComposerCapsules, type InlineAttachmentSink } from "./useComposerCapsules";
import { draftHasPendingUploads } from "./composerUploadModel";
import { classifyAttachmentPaths } from "./composerFiles";
import { ComposerMentionPicker, useComposerMentions } from "./ComposerMentionPicker";
import { useDsTasksExpander } from "./dsTasks/useDsTasksExpander";
import { ReplyQuote } from "@codeterm/chat-ui";
import type { ComposerReply } from "./composerReply";
import { useComposerReply } from "./useComposerReply";

const VOICE_TRANSCRIBING_PLACEHOLDER = "🎙 Transcribing voice note…";
const VOICE_TRANSCRIBE_ERROR = "⚠️ Voice transcription failed — tap the mic to retry.";
const CLI_FALLBACK_COMMANDS = new Set([
  "/usage",
  "/config",
  "/doctor",
  "/mcp",
  "/hooks",
  "/theme",
  "/permissions",
  "/help",
]);

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

function cliFallbackCommandPayload(text: string): string | null {
  const trimmed = text.trim();
  if (!CLI_FALLBACK_COMMANDS.has(trimmed)) return null;
  return `${trimmed}\r`;
}

function SlashPicker({
  suggestions,
  highlighted,
  onHighlight,
  onPick,
}: {
  suggestions: SlashCommandSuggestion[];
  highlighted: number;
  onHighlight: (index: number) => void;
  onPick: (cmd: SlashCommandSuggestion) => void;
}) {
  if (suggestions.length === 0) return null;
  return (
    <div
      role="listbox"
      aria-label="Command suggestions"
      className="ct-menu absolute bottom-full left-0 right-0 mb-1 max-h-56 overflow-y-auto"
      style={SUGGESTION_MENU_STYLE}
    >
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
  );
}

const SUGGESTION_MENU_STYLE = { zIndex: 10, "--ct-menu-origin": "bottom left", "--ct-menu-shift": "6px" } as CSSProperties;

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
  currentPtyId,
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
  onOpenTerminalFallback,
  promptDock,
  replacement,
  inlineAttachmentSink,
  contextSlot = null,
  chatBackend = false,
  isActive = true,
}: {
  tabId: string;
  tabProvider: string | null;
  currentPtyId: string | null;
  /** This tab is backed by a plugin chatBackend (no PTY) — send via the
   *  tab-level `chat_send_text` instead of writing to a PTY. */
  chatBackend?: boolean;
  /** Whether this composer's tab is the currently active/focused tab.
   *  When false, refocusComposer is a no-op — prevents background tabs
   *  from stealing focus after an async send completes. */
  isActive?: boolean;
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
  onOpenTerminalFallback?: () => void;
  promptDock?: ReactNode;
  replacement?: ReactNode;
  /** Where pasted and dropped files land as capsules at the caret. */
  inlineAttachmentSink?: MutableRefObject<InlineAttachmentSink | null>;
  /** Element above the status line that hosts the context header. */
  contextSlot?: HTMLElement | null;
}) {
  recordComposerRender(tabId);
  const isActiveRef = useRef(isActive);
  isActiveRef.current = isActive;

  const [input, setInput] = usePersistentDraft(`tab-session-chat:${tabId}`);
  const recall = useComposerRecall(tabId);
  const workspaceId = useMuxSelector(useCallback((state) => state.workspaces.find((workspace) => workspace.tabs.includes(tabId))?.id ?? state.tabs[tabId]?.workspace_id ?? null, [tabId]));
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerHighlight, setPickerHighlight] = useState(0);
  const touchMode = useTouchOptimizedUi();
  const composerDevice = classifyComposerDevice({
    userAgent: navigator.userAgent, platform: navigator.platform,
    maxTouchPoints: navigator.maxTouchPoints, touchOptimizedUi: touchMode,
  });
  const compositionGate = useRef(createCompositionGate()).current;
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const shellRef = useRef<HTMLDivElement>(null);
  const { layout: composerLayout, measure: measureComposer } = useComposerLayout(input, textareaRef, shellRef, compositionGate, !replacement);
  const lastSendRef = useRef<{ text: string; at: number }>({ text: "", at: 0 });
  const pendingDraftsRef = useRef(new Set<string>());
  const [delayedMessages, setDelayedMessages] = useState<DelayedMessage[]>([]);
  const [delayedBusyId, setDelayedBusyId] = useState<string | null>(null);
  const [delayedModal, setDelayedModal] = useState<DelayedModalTarget | null>(null);
  const [scheduleBusy, setScheduleBusy] = useState(false);
  // Screen-reader-visible mirror of showComposerError: the field's current
  // content was rejected, until the user edits it again.
  const [composerInvalid, setComposerInvalid] = useState(false);
  const flagComposerError = useCallback(
    (message: string) => {
      setComposerInvalid(true);
      showComposerError(message);
    },
    [showComposerError],
  );
  const capsules = useComposerCapsules({ tabId, textareaRef, sinkRef: inlineAttachmentSink, onError: flagComposerError });
  const [composing, setComposing] = useState(false);
  const capsulesActive = !composing && hasCapsuleTokens(input);
  const pendingUploads = draftHasPendingUploads(input, capsules.registry);
  const attachInline = async (paths: string[]) => capsules.insert(await classifyAttachmentPaths(paths));
  const capabilities = useMuxSelector(useCallback(state => state.tabs[tabId]?.provider_signals?.capabilities, [tabId]));
  const firstLine = input.split("\n")[0] || "";
  const suggestions = useMemo(
    () => filterSlashCommands(firstLine, cachedSlashCommands(capabilities)),
    [firstLine, capabilities],
  );

  const refocusComposer = useCallback(() => {
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
  const reply = useComposerReply(tabId, flagComposerError, refocusComposer);
  const mentionPicker = useComposerMentions(input, setInput, refocusComposer);
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
  const refreshDelayedMessages = useCallback(async () => {
    await delayedFeedRef.current?.refresh();
  }, []);

  useEffect(() => {
    if (delayedTabRef.current !== tabId) setDelayedMessages([]);
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
    const wake = () => {
      if (document.visibilityState !== "hidden") void feed.wake();
    };
    const stopResync = onResync(() => void feed.wake());
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

  function buildDraft(extraAttachments: string[] = []) {
    if (draftHasPendingUploads(textareaRef.current?.value ?? input, capsules.registryRef.current)) return;
    const text = input.trim();
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
    );
  }

  async function send(extraAttachments: string[] = []) {
    const draft = buildDraft(extraAttachments);
    if (!draft) return;
    const { text, allAttachments, recordedVoiceAttachments, quoted } = draft;
    const dupKey = `${text} ${allAttachments.join("|")} ${quoted?.path ?? ""}`;
    const duplicate = pendingDraftsRef.current.has(dupKey)
      || isDuplicateSend(lastSendRef.current, dupKey, Date.now());
    const portable = capsules.toPortable(text);
    const registry = capsules.registryRef.current;
    setInput("");
    if (textareaRef.current) textareaRef.current.value = "";
    clearAttachments();
    capsules.clear();
    reply.clearIfSent(quoted);
    if (duplicate) return;
    lastSendRef.current = { text: dupKey, at: Date.now() };
    pendingDraftsRef.current.add(dupKey);
    onWillSendMessage(text);
    const optimisticId = addOptimistic(recordedVoiceAttachments.length > 0
      ? VOICE_TRANSCRIBING_PLACEHOLDER : portable || allAttachments.join(" "));
    const deliveryDraft = { text, attachments: allAttachments, recordedVoiceAttachments, capsules: registry, reply: quoted };
    setChatSendDraft(tabId, optimisticId, deliveryDraft);
    setComposerInvalid(false);
    refocusComposer();
    let composed: string;
    try {
      const send = readChatSends(tabId).find((item) => item.id === optimisticId)
        ?? { id: optimisticId, content: portable, sentAtMs: Date.now(), draft: deliveryDraft };
      composed = await prepareChatDelivery(tabId, send, expandDsTasks);
      updateOptimistic(optimisticId, composed);
    } catch {
      failChatSendRequest(tabId, optimisticId);
      pendingDraftsRef.current.delete(dupKey);
      return;
    }
    try {
      const fallbackCommand = !chatBackend && allAttachments.length === 0
        ? cliFallbackCommandPayload(composed) : null;
      if (fallbackCommand) {
        if (!currentPtyId) throw new Error("Tab has no PTY");
        onOpenTerminalFallback?.();
        await invoke("write_pty_string", { ptyId: currentPtyId, data: fallbackCommand, raw: false });
      } else {
        observeChatSendStatus(await sendTrackedChatMessage(tabId, optimisticId, composed));
      }
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
    setScheduleBusy(true);
    try {
      const { composed, transcribeFailed } = await composeDraft(
        body,
        draft.allAttachments,
        draft.recordedVoiceAttachments,
        draft.quoted,
      );
      if (transcribeFailed) {
        flagComposerError(VOICE_TRANSCRIBE_ERROR);
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
      flagComposerError(e instanceof Error ? e.message : String(e));
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

  const openScheduleModal = () => setDelayedModal({ kind: "create", text: input.trim() });

  if (replacement) return <ComposerDock mode={composerLayout.mode} ref={shellRef}>{replacement}</ComposerDock>;

  return (
    <ComposerDock mode={composerLayout.mode} ref={shellRef}>
      {contextSlot && createPortal(
        <ComposerContextHeader
          tabId={tabId}
          workspaceId={workspaceId}
          delayed={delayedMessages}
          onEditDelayed={(message) => setDelayedModal({ kind: "edit", message })}
          onDeleteDelayed={(id) => void withDelayedBusy(id, async () => {
            await deleteDelayedMessage(id);
          })}
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
      <ComposerContextMeter tabId={tabId} />
      <ProviderCapabilityBadge tabId={tabId} />
      </div>
      {promptDock}
      <div data-testid="composer-row" className="relative">
        <ComposerActions
          tabId={tabId}
          onAttach={(paths) => void attachInline(paths)}
          uploadFiles={capsules.uploads.addFiles}
          onSend={() => void send()}
          onSchedule={openScheduleModal}
          onSendWithAttachments={(paths) => void send(paths)}
          onError={flagComposerError}
          sendDisabled={pendingUploads || (!input.trim() && attachments.length === 0 && !reply.reply)}
          hasText={input.trim().length > 0 || attachments.length > 0 || reply.reply !== null}
          cardHeader={(reply.reply || attachments.length > 0) && <>
            {reply.reply && <ReplyQuote label={reply.reply.label} excerpt={reply.reply.excerpt} onRemove={() => { reply.clear(); refocusComposer(); }} />}
            {attachments.length > 0 && <ComposerBubbleRow attachments={attachments} onRemove={removeAttachment} />}
          </>}
        >
          <SlashPicker
            suggestions={pickerOpen ? suggestions : []}
            highlighted={pickerHighlight}
            onHighlight={setPickerHighlight}
            onPick={pickSuggestion}
          />
          <ComposerMentionPicker
            mentions={mentionPicker.mentions}
            highlighted={mentionPicker.highlighted}
            onHighlight={mentionPicker.setHighlighted}
            onPick={mentionPicker.pick}
          />
          {plansSink}
          <ComposerCapsuleField
            tabId={tabId}
            value={input}
            registry={capsules.registry}
            active={capsulesActive}
            textareaRef={textareaRef}
            marginPx={composerLayout.editorPx > COMPOSER_MIN_HEIGHT ? 8 : 0}
            onRemove={capsules.remove}
            uploads={capsules.uploads.uploads}
            onRetryUpload={(id) => void capsules.uploads.retry(id)}
          >
            <ComposerTextInput
              data-testid="chat-composer"
              data-tab-id={tabId}
              aria-label="Message composer"
              aria-invalid={composerInvalid}
              ref={textareaRef}
              value={input}
              onChange={(e) => {
                setComposerInvalid(false);
                recall.reset();
                setInput(e.target.value);
              }}
              onBeforeInput={(e) => {
                recordComposerBeforeInput(e.nativeEvent.inputType || "beforeinput");
              }}
              onKeyDown={(e) => {
                recordComposerIgnoredKey(e.key);
                if (isImeKeyEvent(compositionGate, e.nativeEvent.isComposing, e.nativeEvent.keyCode)) return;
                const enterIntent = composerEnterIntent(composerDevice, {
                  code: e.code, key: e.key, keyCode: e.nativeEvent.keyCode, shiftKey: e.shiftKey, ctrlKey: e.ctrlKey,
                  metaKey: e.metaKey, altKey: e.altKey, isComposing: e.nativeEvent.isComposing,
                });
                if (isComposerEnterKey(e)) {
                  if (enterIntent !== "submit") {
                    if (enterIntent === "newline") setPickerOpen(false);
                    return;
                  }
                }
                if (capsules.handleKeyDown(e)) return;
                if (mentionPicker.handleKey(e)) return;
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
                    if (enterIntent === "submit" && cliFallbackCommandPayload(input)) {
                      void send();
                      return;
                    }
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
                  void send();
                }
              }}
              onCompositionStart={() => {
                beginComposition(compositionGate);
                if (capsulesActive) setComposing(true);
                if (typeof window !== "undefined") window.__composerPerf?.compositionEvents.push("start");
              }}
              onCompositionEnd={(e) => {
                endComposition(compositionGate);
                setComposing(false);
                // Commit the finished draft once and run the autosize the gate
                // deferred; both are no-ops when state already mirrors the DOM.
                setInput(e.currentTarget.value);
                measureComposer();
                if (typeof window !== "undefined") window.__composerPerf?.compositionEvents.push("end");
              }}
              onPaste={(e) => {
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
              onCopy={(e) => capsules.handleCopy(e, false)}
              onCut={(e) => capsules.handleCopy(e, true)}
              onSelect={(e) => capsules.snapSelection(e.currentTarget)}
              placeholder={pendingPrompt
                ? "Agent may be waiting in the terminal."
                : "Message"}
              enterKeyHint={composerDevice === "phone" ? "enter" : "send"}
              style={{
                color: capsulesActive ? "transparent" : "var(--ct-fg, #c9d1d9)",
                lineHeight: `${COMPOSER_MIN_HEIGHT}px`,
                // Margin, not padding: padding would feed back into the scrollHeight autosize.
                margin: composerLayout.editorPx > COMPOSER_MIN_HEIGHT ? "8px 0" : 0,
              }}
            />
          </ComposerCapsuleField>
        </ComposerActions>
      </div>
    </ComposerDock>
  );
}

export const ChatComposer = memo(ChatComposerImpl, (a, b) =>
  a.tabId === b.tabId
  && a.tabProvider === b.tabProvider
  && a.currentPtyId === b.currentPtyId
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
  && a.onOpenTerminalFallback === b.onOpenTerminalFallback
  && a.replacement === b.replacement
  && a.promptDock === b.promptDock
  && a.inlineAttachmentSink === b.inlineAttachmentSink
  && a.contextSlot === b.contextSlot
  && a.chatBackend === b.chatBackend
  && a.isActive === b.isActive
);
