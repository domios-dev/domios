import {ProviderCapabilityBadge} from "./ProviderCapabilityBadge";
import {cachedSlashCommands} from "../../lib/providerCapabilities";
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type MutableRefObject, type ReactNode } from "react";
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
import {
  createDelayedMessage,
  deleteDelayedMessage,
  listDelayedMessages,
  sendDelayedMessageNow,
  updateDelayedMessage,
  type DelayedMessage,
  type RecurrenceRule,
} from "./delayedSend";
import { DelayedSendQueuePanel, DelayedSendSchedulePanel } from "./DelayedSendPanel";
import {
  filterSlashCommands,
  type SlashCommandSuggestion,
} from "../../lib/slashCommands";
import type { PendingPrompt } from "../../lib/types";
import { TabTaskStrip } from "./TabTaskStrip";
import { ComposerJobChips } from "./ComposerJobChips";
import { useMuxSelector } from "../../store/multiplexer";
import { COMPOSER_MIN_HEIGHT } from "./composerAutosize";
import {
  beginComposition,
  createCompositionGate,
  endComposition,
  isImeKeyEvent,
} from "./composerComposition";
import { sendTrackedChatMessage, type ChatSendStatus } from "./chatSendStatus";
import { composeAttachmentDraft } from "./meshAttachmentHandoff";
import { composerAttachmentHandoff } from "./composerAttachmentHandoff";
import { ReceiverTargetMarks } from "./ReceiverTargetMark";
import { usePluginTabMenu } from "../../lib/pluginTabMenu";
import { useComposerInsertions } from "./useComposerInsertions";
import { ComposerCapsuleField } from "./ComposerCapsuleField";
import { hasCapsuleTokens } from "./composerCapsules";
import { useComposerCapsules, type InlineAttachmentSink } from "./useComposerCapsules";

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
  onPick,
}: {
  suggestions: SlashCommandSuggestion[];
  highlighted: number;
  onPick: (cmd: SlashCommandSuggestion) => void;
}) {
  if (suggestions.length === 0) return null;
  return (
    <div
      className="absolute bottom-full left-0 right-0 mb-1 max-h-48 overflow-y-auto rounded"
      style={{
        background: "color-mix(in srgb, var(--ct-bg, #0d1117) 90%, black 10%)",
        border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 30%, transparent)",
        zIndex: 10,
      }}
    >
      {suggestions.map((c, i) => (
        <button
          key={c.name}
          type="button"
          data-testid="slash-command-option"
          data-slash-command={c.name}
          onClick={() => onPick(c)}
          className="w-full flex items-center justify-between gap-2 px-2 py-1 text-xs font-mono text-left"
          style={{
            background: i === highlighted
              ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 20%, transparent)"
              : "transparent",
            color: "var(--ct-fg, #c9d1d9)",
            border: "none",
            cursor: "pointer",
          }}
        >
          <span style={{ color: "var(--ct-accent, #58a6ff)" }}>{c.name}{c.argumentHint ? ` ${c.argumentHint}` : ""}</span>
          <span className="opacity-60 truncate">{c.description}</span>
        </button>
      ))}
    </div>
  );
}

/** Poll cadence for delayed-message pills: only the active tab polls. */
export function delayedPollIntervalMs(isActive: boolean): number | null {
  return isActive ? 10_000 : null;
}

export function composerEnterIntent(
  key: string,
  shiftKey: boolean,
  isComposing: boolean,
): "submit" | "newline" | "none" {
  if (isComposing) return "none";
  if (key !== "Enter") return "none";
  return shiftKey ? "newline" : "submit";
}

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
  addAttachments,
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
  inlineAttachmentSink,
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
  /** Where pasted and dropped files land as capsules at the caret. */
  inlineAttachmentSink?: MutableRefObject<InlineAttachmentSink | null>;
}) {
  recordComposerRender(tabId);
  const isActiveRef = useRef(isActive);
  isActiveRef.current = isActive;
  const pluginTabMenu = usePluginTabMenu();

  const [input, setInput] = usePersistentDraft(`tab-session-chat:${tabId}`);
  const recall = useComposerRecall(tabId);
  const workspaceId = useMuxSelector(useCallback((state) => state.workspaces.find((workspace) => workspace.tabs.includes(tabId))?.id ?? state.tabs[tabId]?.workspace_id ?? null, [tabId]));
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerHighlight, setPickerHighlight] = useState(0);
  const touchMode = useTouchOptimizedUi();
  const compositionGate = useRef(createCompositionGate()).current;
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const currentDraftRef = useRef({ input, attachments, revision: 0 });
  const currentDraft = currentDraftRef.current;
  if (currentDraft.input !== input || currentDraft.attachments !== attachments) {
    currentDraftRef.current = { input, attachments, revision: currentDraft.revision + 1 };
  }
  const shellRef = useRef<HTMLDivElement>(null);
  const { layout: composerLayout, measure: measureComposer } = useComposerLayout(input, textareaRef, shellRef, compositionGate);
  const lastSendRef = useRef<{ text: string; at: number }>({ text: "", at: 0 });
  const [delayedMessages, setDelayedMessages] = useState<DelayedMessage[]>([]);
  const [delayedQueueOpen, setDelayedQueueOpen] = useState(false);
  const [scheduleOpen, setScheduleOpen] = useState(false);
  const [delayedBusyId, setDelayedBusyId] = useState<string | null>(null);
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

  const refreshDelayedMessages = useCallback(async () => {
    try {
      setDelayedMessages(await listDelayedMessages(tabId));
    } catch {
      setDelayedMessages([]);
    }
  }, [tabId]);

  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      try {
        const messages = await listDelayedMessages(tabId);
        if (!cancelled) setDelayedMessages(messages);
      } catch {
        if (!cancelled) setDelayedMessages([]);
      }
    };
    void refresh();
    // Change events drive updates; the interval is drift insurance for the
    // countdown pills and runs only while this tab is the active tab.
    const pollMs = delayedPollIntervalMs(isActive);
    const interval = pollMs !== null ? window.setInterval(refresh, pollMs) : null;
    let unlisten: (() => void) | null = null;
    void listen<{ tabId?: string }>("delayed-send-changed", (e) => {
      if (!e.payload?.tabId || e.payload.tabId === tabId) void refresh();
    }).then((fn) => {
      unlisten = fn;
    }).catch(() => {});
    return () => {
      cancelled = true;
      if (interval !== null) window.clearInterval(interval);
      unlisten?.();
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
    const text = input.trim();
    const cleanExtraAttachments = extraAttachments
      .map((p) => p.trim())
      .filter((p, i, all) => p.length > 0 && all.indexOf(p) === i);
    const allAttachments = [...attachments];
    for (const p of cleanExtraAttachments) {
      if (!allAttachments.includes(p)) allAttachments.push(p);
    }
    if (!text && allAttachments.length === 0) return;
    return { text, allAttachments, recordedVoiceAttachments: cleanExtraAttachments };
  }

  async function composeDraft(
    text: string,
    allAttachments: string[],
    recordedVoiceAttachments: string[],
  ) {
    return composeAttachmentDraft(
      tabId,
      text,
      allAttachments,
      recordedVoiceAttachments,
      composerAttachmentHandoff,
      capsules.registryRef.current,
    );
  }

  async function send(extraAttachments: string[] = []) {
    const draft = buildDraft(extraAttachments);
    if (!draft) return;
    const submittedDraft = currentDraftRef.current;
    const { text, allAttachments, recordedVoiceAttachments } = draft;
    const nowTs = Date.now();
    const dupKey = `${text} ${allAttachments.join("|")}`;
    if (isDuplicateSend(lastSendRef.current, dupKey, nowTs)) {
      console.warn("[chat-send] duplicate optimistic send suppressed", { tabId });
      return;
    }
    lastSendRef.current = { text: dupKey, at: nowTs };
    const placeholderId = recordedVoiceAttachments.length > 0
      ? addOptimistic(VOICE_TRANSCRIBING_PLACEHOLDER)
      : null;
    let draftResult: Awaited<ReturnType<typeof composeDraft>>;
    try {
      draftResult = await composeDraft(text, allAttachments, recordedVoiceAttachments);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      if (placeholderId) updateOptimistic(placeholderId, `⚠️ ${message}`);
      else flagComposerError(message);
      refocusComposer();
      return;
    }
    const { composed, transcribeFailed } = draftResult;
    if (placeholderId && transcribeFailed) {
      updateOptimistic(placeholderId, VOICE_TRANSCRIBE_ERROR);
      setInput("");
      clearAttachments();
      capsules.clear();
      refocusComposer();
      return;
    }
    let optimisticId: string;
    if (placeholderId) {
      updateOptimistic(placeholderId, composed);
      optimisticId = placeholderId;
    } else {
      onWillSendMessage(composed);
      optimisticId = addOptimistic(composed);
    }
    setComposerInvalid(false);
    refocusComposer();
    try {
      if (chatBackend) {
        // plugin's sendMessage (chat_send_text → send_text_when_ready).
        observeChatSendStatus(await sendTrackedChatMessage(tabId, optimisticId, composed));
      } else {
        const fallbackCommand = allAttachments.length === 0 ? cliFallbackCommandPayload(composed) : null;
        if (fallbackCommand) {
          // CLI fallback commands write directly to the PTY — need the id now.
          if (!currentPtyId) throw new Error("Tab has no PTY");
          onOpenTerminalFallback?.();
          await invoke("write_pty_string", { ptyId: currentPtyId, data: fallbackCommand, raw: false });
        } else {
          observeChatSendStatus(await sendTrackedChatMessage(tabId, optimisticId, composed));
        }
      }
      recall.remember(capsules.toPortable(text));
      if (currentDraftRef.current.revision === submittedDraft.revision
        && textareaRef.current?.value === submittedDraft.input) {
        setInput("");
        clearAttachments();
        capsules.clear();
      }
    } catch (e) {
      console.warn("[chat-send] optimistic delivery failed", {
        tabId,
        messageId: optimisticId,
        error: e instanceof Error ? e.message : String(e),
      });
    } finally {
      refocusComposer();
    }
  }

  async function scheduleDraft(scheduledAtMs: number | null, recurrence?: RecurrenceRule) {
    const draft = buildDraft();
    if (!draft) return;
    setScheduleBusy(true);
    try {
      const { composed, transcribeFailed } = await composeDraft(
        draft.text,
        draft.allAttachments,
        draft.recordedVoiceAttachments,
      );
      if (transcribeFailed) {
        flagComposerError(VOICE_TRANSCRIBE_ERROR);
        return;
      }

      await createDelayedMessage(tabId, composed, scheduledAtMs, recurrence);
      setInput("");
      clearAttachments();
      capsules.clear();
      setScheduleOpen(false);
      setDelayedQueueOpen(true);
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

  const queuedCount = delayedMessages.filter((msg) => msg.state !== "failed").length;
  const failedCount = delayedMessages.filter((msg) => msg.state === "failed").length;

  useEffect(() => {
    if (delayedQueueOpen && delayedMessages.length === 0) {
      setDelayedQueueOpen(false);
    }
  }, [delayedQueueOpen, delayedMessages.length]);

  function openDelayedQueue() {
    setDelayedQueueOpen((value) => !value);
    setScheduleOpen(false);
  }

  return (
    <ComposerDock mode={composerLayout.mode} ref={shellRef}>
      {(scheduleOpen || delayedQueueOpen) && (
        <div className="absolute bottom-full left-0 z-30">
          {scheduleOpen && (
            <DelayedSendSchedulePanel
              busy={scheduleBusy}
              touchMode={touchMode}
              onClose={() => {
                setScheduleOpen(false);
                refocusComposer();
              }}
              onConfirm={(scheduledAtMs, recurrence) => void scheduleDraft(scheduledAtMs, recurrence)}
              onStash={() => void scheduleDraft(null)}
            />
          )}
          {delayedQueueOpen && (
            <DelayedSendQueuePanel
              messages={delayedMessages}
              busyId={delayedBusyId}
              touchMode={touchMode}
              onClose={() => {
                setDelayedQueueOpen(false);
                refocusComposer();
              }}
              onDelete={(id) => void withDelayedBusy(id, async () => {
                await deleteDelayedMessage(id);
              })}
              onSendNow={(id) => void withDelayedBusy(id, async () => {
                await sendDelayedMessageNow(id);
              })}
              onUpdate={(id, patch) => void withDelayedBusy(id, async () => {
                await updateDelayedMessage(id, patch);
              })}
            />
          )}
        </div>
      )}
      <ReceiverTargetMarks menu={pluginTabMenu} tabId={tabId} surface="composer" />
      <div className="mb-2 flex flex-wrap items-center gap-1 empty:hidden">
      <ComposerContextMeter tabId={tabId} />
      <ProviderCapabilityBadge tabId={tabId} />
      <TabTaskStrip tabId={tabId} presentation="inline" />
      <ComposerJobChips tabId={tabId} workspaceId={workspaceId} />
      </div>
      {promptDock}
      <div data-testid="composer-row" className="relative">
        <ComposerActions
          onAttach={addAttachments}
          onSend={() => void send()}
          onSchedule={() => {
            setScheduleOpen((v) => !v);
            setDelayedQueueOpen(false);
          }}
          onSendWithAttachments={(paths) => void send(paths)}
          onError={flagComposerError}
          sendDisabled={!input.trim() && attachments.length === 0}
          hasText={input.trim().length > 0 || attachments.length > 0}
          cardHeader={attachments.length > 0 && <ComposerBubbleRow
            attachments={attachments}
            onRemove={removeAttachment}
            queuedCount={0}
            failedCount={0}
            onOpenDelayedQueue={openDelayedQueue}
          />}
          cardFooter={(queuedCount > 0 || failedCount > 0) && <ComposerBubbleRow
            attachments={[]}
            onRemove={removeAttachment}
            queuedCount={queuedCount}
            failedCount={failedCount}
            onOpenDelayedQueue={openDelayedQueue}
          />}
        >
          <SlashPicker
            suggestions={pickerOpen ? suggestions : []}
            highlighted={pickerHighlight}
            onPick={pickSuggestion}
          />
          <ComposerCapsuleField
            tabId={tabId}
            value={input}
            registry={capsules.registry}
            active={capsulesActive}
            textareaRef={textareaRef}
            marginPx={composerLayout.editorPx > COMPOSER_MIN_HEIGHT ? 8 : 0}
            onRemove={capsules.remove}
          >
            <textarea
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
                if (touchMode && e.key === "Enter" && !e.shiftKey) {
                  setPickerOpen(false);
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
                  if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey)) {
                    e.preventDefault();
                    if (e.key === "Enter" && cliFallbackCommandPayload(input)) {
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
                if (composerEnterIntent(e.key, e.shiftKey, e.nativeEvent.isComposing) === "submit") {
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
                void handlePasteEvent(e.nativeEvent);
              }}
              onCopy={(e) => capsules.handleCopy(e, false)}
              onCut={(e) => capsules.handleCopy(e, true)}
              onSelect={(e) => capsules.snapSelection(e.currentTarget)}
              placeholder={pendingPrompt
                ? "Agent may be waiting in the terminal."
                : "Message"}
              rows={1}
              autoComplete="off"
              autoCorrect="off"
              autoCapitalize="sentences"
              spellCheck={false}
              enterKeyHint={touchMode ? "enter" : "send"}
              inputMode="text"
              className="block w-full min-h-5 resize-none bg-transparent py-0 pr-1 text-[15px] leading-5 outline-none placeholder:text-[var(--ct-muted-fg)]"
              style={{
                color: capsulesActive ? "transparent" : "var(--ct-fg, #c9d1d9)",
                caretColor: "var(--ct-fg, #c9d1d9)",
                border: "none",
                lineHeight: `${COMPOSER_MIN_HEIGHT}px`,
                // Margin, not padding: padding would feed back into the scrollHeight autosize.
                margin: composerLayout.editorPx > COMPOSER_MIN_HEIGHT ? "8px 0" : 0,
                overflowY: "auto",
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
  && a.promptDock === b.promptDock
  && a.inlineAttachmentSink === b.inlineAttachmentSink
  && a.chatBackend === b.chatBackend
  && a.isActive === b.isActive
);
