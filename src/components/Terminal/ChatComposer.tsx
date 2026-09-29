import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Clock3, Paperclip, X } from "lucide-react";
import { ComposerActions } from "./ComposerActions";
import { isDuplicateSend } from "./chatSendGuard";
import { invoke, listen } from "../../lib/bridge";
import { supportsLiveModelSwitch } from "../../lib/modelSwitch";
import { getTabModelApi } from "../Sidebar/tabModelApi";
import { isModelSelectable, modelAvailability } from "../../lib/modelAvailability";
import { usePersistentDraft } from "../../lib/usePersistentDraft";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { useAgentProviders } from "../../hooks/useAgentProviders";
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
import type { ModelInfo, PendingPrompt } from "../../lib/types";
import { ModelSuggestionPicker } from "./ModelSuggestionPicker";
import { TabTaskStrip } from "./TabTaskStrip";
import { COMPOSER_MIN_HEIGHT, applyComposerAutosize } from "./composerAutosize";
import {
  beginComposition,
  compositionDefersAutosize,
  createCompositionGate,
  endComposition,
  isImeKeyEvent,
} from "./composerComposition";
import { sendTrackedChatMessage, type ChatSendStatus } from "./chatSendStatus";
import { composeAttachmentDraft } from "./meshAttachmentHandoff";
import { composerAttachmentHandoff } from "./composerAttachmentHandoff";
import { ReceiverTargetMarks } from "./ReceiverTargetMark";
import { usePluginTabMenu } from "../../lib/pluginTabMenu";

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

function AttachmentChip({ path, onRemove }: { path: string; onRemove: () => void }) {
  const name = path.split(/[\\/]/).pop() || path;
  return (
    <div
      data-testid="attachment-chip"
      className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs font-mono"
      style={{
        background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, transparent)",
        color: "var(--ct-fg, #c9d1d9)",
        border: "1px solid color-mix(in srgb, var(--ct-accent, #58a6ff) 35%, transparent)",
      }}
    >
      <Paperclip size={10} className="opacity-70" />
      <span className="max-w-[200px] truncate" title={path}>{name}</span>
      <button
        type="button"
        onClick={onRemove}
        className="opacity-60 hover:opacity-100"
        style={{ background: "transparent", border: "none", padding: 0, cursor: "pointer", color: "inherit" }}
        title="Remove"
      >
        <X size={10} />
      </button>
    </div>
  );
}

function ComposerBubbleRow({
  attachments,
  onRemove,
  queuedCount,
  failedCount,
  onOpenDelayedQueue,
  modelBubble,
}: {
  attachments: string[];
  onRemove: (path: string) => void;
  queuedCount: number;
  failedCount: number;
  onOpenDelayedQueue: () => void;
  modelBubble?: ReactNode;
}) {
  if (attachments.length === 0 && queuedCount === 0 && failedCount === 0 && !modelBubble) return null;
  const visible = attachments.slice(0, 4);
  const overflow = attachments.length - visible.length;
  return (
    <div
      data-testid="composer-bubble-row"
      className="mb-2 flex max-h-[3.6rem] flex-wrap items-center gap-1 overflow-visible"
    >
      {modelBubble}
      {(queuedCount > 0 || failedCount > 0) && (
        <button
          type="button"
          data-testid="delayed-send-bubble"
          onMouseDown={(e) => e.preventDefault()}
          onClick={onOpenDelayedQueue}
          className="inline-flex h-7 items-center gap-1 rounded-full border px-2 text-xs font-mono hover:brightness-110"
          style={{
            background: failedCount > 0
              ? "color-mix(in srgb, var(--ct-red, #f85149) 12%, transparent)"
              : "color-mix(in srgb, var(--ct-accent, #58a6ff) 12%, transparent)",
            borderColor: failedCount > 0
              ? "color-mix(in srgb, var(--ct-red, #f85149) 34%, transparent)"
              : "color-mix(in srgb, var(--ct-accent, #58a6ff) 30%, transparent)",
            color: failedCount > 0 ? "var(--ct-red, #f85149)" : "var(--ct-muted-fg)",
          }}
          title="Queued messages"
        >
          <Clock3 size={12} />
          <span>{queuedCount > 0 ? `${queuedCount} queued` : `${failedCount} failed`}</span>
        </button>
      )}
      {visible.map((p) => (
        <AttachmentChip key={p} path={p} onRemove={() => onRemove(p)} />
      ))}
      {overflow > 0 && (
        <span
          data-testid="attachment-overflow-bubble"
          className="inline-flex h-7 items-center rounded-full border px-2 text-xs font-mono"
          style={{
            background: "color-mix(in srgb, var(--ct-accent, #58a6ff) 10%, transparent)",
            borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 28%, transparent)",
            color: "var(--ct-muted-fg)",
          }}
        >
          and {overflow} more
        </span>
      )}
    </div>
  );
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
          <span style={{ color: "var(--ct-accent, #58a6ff)" }}>{c.name}</span>
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
  tabProvider,
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
  modelBubble,
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
  onWillSendMessage: () => void;
  addOptimistic: (content: string) => string;
  updateOptimistic: (id: string, content: string) => void;
  observeChatSendStatus: (status: ChatSendStatus) => void;
  showComposerError: (message: string) => void;
  onOpenTerminalFallback?: () => void;
  modelBubble?: ReactNode;
}) {
  recordComposerRender(tabId);
  const isActiveRef = useRef(isActive);
  isActiveRef.current = isActive;
  const pluginTabMenu = usePluginTabMenu();

  const [input, setInput] = usePersistentDraft(`tab-session-chat:${tabId}`);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerHighlight, setPickerHighlight] = useState(0);
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const [modelHighlight, setModelHighlight] = useState(0);
  const { providers, loading: modelCatalogLoading } = useAgentProviders();
  const providerDescriptor = useMemo(
    () => providers.find((provider) => provider.id === tabProvider),
    [providers, tabProvider],
  );
  const modelOptions = useMemo(
    () => providerDescriptor?.available_models ?? [],
    [providerDescriptor],
  );
  // Opening the picker is an explicit user action, not a poll: it asks the
  // catalog to re-probe. The backend TTL-gates the actual probe.
  useEffect(() => {
    if (!modelPickerOpen || !tabProvider) return;
    void invoke("refresh_provider_models", { providerId: tabProvider }).catch(() => {});
  }, [modelPickerOpen, tabProvider]);
  const touchMode = useTouchOptimizedUi();
  const compositionGate = useRef(createCompositionGate()).current;
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const shellRef = useRef<HTMLDivElement>(null);
  const lastSendRef = useRef<{ text: string; at: number }>({ text: "", at: 0 });
  const [delayedMessages, setDelayedMessages] = useState<DelayedMessage[]>([]);
  const [delayedQueueOpen, setDelayedQueueOpen] = useState(false);
  const [scheduleOpen, setScheduleOpen] = useState(false);
  const [delayedBusyId, setDelayedBusyId] = useState<string | null>(null);
  const [scheduleBusy, setScheduleBusy] = useState(false);
  const [modelSuggestionDismissedFor, setModelSuggestionDismissedFor] = useState<string | null>(null);
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
  const firstLine = input.split("\n")[0] || "";
  const modelCommandQuery = useMemo(() => {
    const match = firstLine.match(/^\/model\s+([^\n]*)$/i);
    return match ? match[1].trim().toLowerCase() : null;
  }, [firstLine]);
  const filteredModelOptions = useMemo(() => {
    if (modelCommandQuery === null) return [];
    if (!supportsLiveModelSwitch(providerDescriptor)) return [];
    const needle = modelCommandQuery;
    if (!needle) return modelOptions;
    return modelOptions.filter((model) => {
      const haystack = `${model.id} ${model.display_name} ${model.description ?? ""}`.toLowerCase();
      return haystack.includes(needle);
    });
  }, [modelCommandQuery, modelOptions, providerDescriptor]);
  const modelSuggestionRequested = modelCommandQuery !== null
    && !modelPickerOpen
    && modelSuggestionDismissedFor !== firstLine;
  const modelSuggestionOpen = modelSuggestionRequested && filteredModelOptions.length > 0;
  const modelSuggestionEmptyMessage = modelCatalogLoading
    ? "Loading model catalog…"
    : modelOptions.length === 0
      ? "No models published"
      : "No matching models";

  const suggestions = useMemo(
    () => filterSlashCommands(firstLine),
    [firstLine],
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
    window.setTimeout(() => {
      if (document.activeElement !== textareaRef.current) {
        focus();
      }
    }, 0);
  }, []);

  useLayoutEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    if (compositionDefersAutosize(compositionGate)) return;
    applyComposerAutosize(el, shellRef.current);
    recordComposerPaint();
  }, [input, compositionGate]);

  useEffect(() => {
    if (modelSuggestionDismissedFor && modelSuggestionDismissedFor !== firstLine) {
      setModelSuggestionDismissedFor(null);
    }
  }, [firstLine, modelSuggestionDismissedFor]);

  useEffect(() => {
    setPickerOpen(
      !modelPickerOpen
        && !modelSuggestionOpen
        && firstLine.startsWith("/")
        && suggestions.length > 0,
    );
    setPickerHighlight(0);
  }, [firstLine, modelPickerOpen, modelSuggestionOpen, suggestions.length]);

  useEffect(() => {
    if (!modelSuggestionOpen) return;
    setModelHighlight((h) => Math.min(Math.max(h, 0), filteredModelOptions.length - 1));
  }, [filteredModelOptions.length, modelSuggestionOpen]);

  useEffect(() => {
    const defaultIndex = providerDescriptor?.default_model
      ? modelOptions.findIndex((model) => model.id === providerDescriptor.default_model)
      : -1;
    setModelHighlight(defaultIndex >= 0 ? defaultIndex : 0);
  }, [modelOptions, providerDescriptor?.default_model]);

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

  const chooseModel = useCallback(
    async (model: ModelInfo) => {
      if (!isModelSelectable(model)) {
        flagComposerError(`${model.display_name} is not available yet.`);
        return;
      }
      setModelPickerOpen(false);
      setInput("");
      try {
        if (!supportsLiveModelSwitch(providerDescriptor)) {
          throw new Error(
            providerDescriptor
              ? `${providerDescriptor.display_name} does not support live model switching.`
              : "This provider does not support live model switching.",
          );
        }
        await getTabModelApi().switchModel(tabId, model.id);
      } catch (e) {
        flagComposerError(e instanceof Error ? e.message : String(e));
      } finally {
        refocusComposer();
      }
    },
    [flagComposerError, providerDescriptor, refocusComposer, setInput, tabId],
  );

  function completeModelDraft(model: ModelInfo) {
    const lines = input.split("\n");
    lines[0] = `/model ${model.id}`;
    setInput(lines.join("\n"));
    setPickerOpen(false);
    setModelPickerOpen(false);
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
    );
  }

  async function send(extraAttachments: string[] = []) {
    const draft = buildDraft(extraAttachments);
    if (!draft) return;
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
      refocusComposer();
      return;
    }
    if (
      allAttachments.length === 0
      && modelCommandQuery !== null
      && filteredModelOptions.length === 0
    ) {
      flagComposerError(modelSuggestionEmptyMessage);
      refocusComposer();
      return;
    }
    if (
      allAttachments.length === 0
      && modelCommandQuery !== null
      && supportsLiveModelSwitch(providerDescriptor)
      && filteredModelOptions.length > 0
    ) {
      const model = filteredModelOptions[Math.min(modelHighlight, filteredModelOptions.length - 1)];
      if (model) {
        await chooseModel(model);
        return;
      }
    }
    if (allAttachments.length === 0 && composed.trim() === "/model") {
      setInput("");
      if (modelOptions.length === 0 || !supportsLiveModelSwitch(providerDescriptor)) {
        flagComposerError(
          providerDescriptor && modelOptions.length === 0
            ? `${providerDescriptor.display_name} does not publish switchable models.`
            : providerDescriptor
              ? `${providerDescriptor.display_name} does not support live model switching.`
              : "This provider does not publish switchable models.",
        );
      } else {
        setModelPickerOpen(true);
      }
      setPickerOpen(false);
      refocusComposer();
      return;
    }
    let optimisticId: string;
    if (placeholderId) {
      updateOptimistic(placeholderId, composed);
      optimisticId = placeholderId;
    } else {
      onWillSendMessage();
      optimisticId = addOptimistic(composed);
    }
    setComposerInvalid(false);
    setInput("");
    clearAttachments();
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
      if (draft.allAttachments.length === 0 && composed.trim() === "/model") {
        flagComposerError("Model picker cannot be scheduled.");
        return;
      }
      await createDelayedMessage(tabId, composed, scheduledAtMs, recurrence);
      setInput("");
      clearAttachments();
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

  return (
    <div
      ref={shellRef}
      className="relative shrink-0 p-2 border-t"
      data-testid="chat-composer-shell"
      style={{
        borderColor:
          "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 30%, transparent)",
        paddingBottom: "var(--ct-composer-bottom-padding, calc(0.5rem + var(--ct-safe-bottom, 0px)))",
      }}
    >
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
      {modelPickerOpen && (
        <div
          role="dialog"
          aria-label="Select model"
          aria-busy={modelCatalogLoading}
          className="mb-2 rounded-md border p-2 shadow-lg"
          style={{
            background: "color-mix(in srgb, var(--ct-bg, #0d1117) 94%, black 6%)",
            borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 35%, transparent)",
          }}
        >
          <div className="flex items-center justify-between gap-2 mb-2">
            <div className="text-xs font-medium truncate" style={{ color: "var(--ct-fg)" }}>
              Select {providerDescriptor?.display_name ?? "provider"} model
            </div>
            <button
              type="button"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                setModelPickerOpen(false);
                refocusComposer();
              }}
              className="px-2 h-7 rounded text-xs font-mono hover:brightness-125"
              style={{ background: "var(--ct-surface)", color: "var(--ct-muted-fg)" }}
              title="Cancel"
            >
              Esc
            </button>
          </div>
          <div className="grid gap-1" role="group" aria-label="Available models">
            {modelCatalogLoading ? (
              <p className="px-2 py-1.5 text-xs text-[var(--ct-muted-fg)]" role="status">
                Loading model catalog…
              </p>
            ) : modelOptions.length === 0 ? (
              <p className="px-2 py-1.5 text-xs text-[var(--ct-muted-fg)]" role="status">
                No models published
              </p>
            ) : modelOptions.map((model, i) => {
              const selected = i === modelHighlight;
              const availability = modelAvailability(model);
              const unavailable = availability === "unavailable";
              const unknown = availability === "unknown";
              return (
                <button
                  key={model.id}
                  type="button"
                  disabled={unavailable}
                  onMouseDown={(e) => e.preventDefault()}
                  onMouseEnter={() => setModelHighlight(i)}
                  onClick={() => void chooseModel(model)}
                  aria-pressed={selected}
                  aria-disabled={unavailable}
                  title={
                    unavailable
                      ? `${model.display_name} is not available yet`
                      : unknown
                        ? `CodeTerm could not check whether ${model.display_name} is available`
                        : undefined
                  }
                  className="w-full grid grid-cols-[1rem_minmax(0,1fr)] gap-2 rounded px-2 py-1.5 text-left text-xs"
                  style={{
                    background: selected && !unavailable
                      ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 18%, transparent)"
                      : "transparent",
                    color: selected && !unavailable ? "var(--ct-fg)" : "var(--ct-muted-fg)",
                    opacity: unavailable ? 0.5 : 1,
                    cursor: unavailable ? "not-allowed" : undefined,
                  }}
                >
                  <span className="text-center font-mono">{selected && !unavailable ? ">" : ""}</span>
                  <span className="min-w-0">
                    <span className="block truncate font-medium">
                      {model.display_name}
                      {unavailable ? " (unavailable)" : unknown ? " (availability unknown)" : ""}
                    </span>
                    <span className="block truncate opacity-60 font-mono">{model.id}</span>
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      )}
      <ReceiverTargetMarks menu={pluginTabMenu} tabId={tabId} surface="composer" />
      <TabTaskStrip tabId={tabId} />
      <ComposerBubbleRow
        attachments={attachments}
        onRemove={removeAttachment}
        queuedCount={queuedCount}
        failedCount={failedCount}
        modelBubble={modelBubble}
        onOpenDelayedQueue={() => {
          setDelayedQueueOpen((v) => !v);
          setScheduleOpen(false);
        }}
      />
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
        >
          <SlashPicker
            suggestions={pickerOpen ? suggestions : []}
            highlighted={pickerHighlight}
            onPick={pickSuggestion}
          />
          <ModelSuggestionPicker
            models={modelSuggestionRequested && !modelCatalogLoading ? filteredModelOptions : []}
            highlighted={modelHighlight}
            defaultModel={providerDescriptor?.default_model}
            emptyMessage={modelSuggestionRequested ? modelSuggestionEmptyMessage : undefined}
            onHighlight={setModelHighlight}
            onPick={(model) => void chooseModel(model)}
          />
          <textarea
            data-testid="chat-composer"
            data-tab-id={tabId}
            aria-label="Message composer"
            aria-invalid={composerInvalid}
            ref={textareaRef}
            value={input}
            onChange={(e) => {
              setComposerInvalid(false);
              setInput(e.target.value);
            }}
            onBeforeInput={(e) => {
              recordComposerBeforeInput(e.nativeEvent.inputType || "beforeinput");
            }}
            onKeyDown={(e) => {
              recordComposerIgnoredKey(e.key);
              if (isImeKeyEvent(compositionGate, e.nativeEvent.isComposing, e.nativeEvent.keyCode)) return;
              if (touchMode && e.key === "Enter" && !e.shiftKey) {
                setPickerOpen(false);
                setModelPickerOpen(false);
                return;
              }
              if (modelPickerOpen) {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setModelHighlight((h) => Math.min(h + 1, modelOptions.length - 1));
                  return;
                }
                if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setModelHighlight((h) => Math.max(h - 1, 0));
                  return;
                }
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  const model = modelOptions[modelHighlight];
                  if (model) void chooseModel(model);
                  return;
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  setModelPickerOpen(false);
                  return;
                }
              }
              if (modelSuggestionOpen) {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setModelHighlight((h) => Math.min(h + 1, filteredModelOptions.length - 1));
                  return;
                }
                if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setModelHighlight((h) => Math.max(h - 1, 0));
                  return;
                }
                if (e.key === "Tab") {
                  e.preventDefault();
                  const model = filteredModelOptions[modelHighlight];
                  if (model) completeModelDraft(model);
                  return;
                }
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  const model = filteredModelOptions[modelHighlight];
                  if (model) void chooseModel(model);
                  return;
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  setModelSuggestionDismissedFor(firstLine);
                  return;
                }
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
              if (typeof window !== "undefined") window.__composerPerf?.compositionEvents.push("start");
            }}
            onCompositionEnd={(e) => {
              endComposition(compositionGate);
              // Commit the finished draft once and run the autosize the gate
              // deferred; both are no-ops when state already mirrors the DOM.
              setInput(e.currentTarget.value);
              applyComposerAutosize(e.currentTarget, shellRef.current);
              if (typeof window !== "undefined") window.__composerPerf?.compositionEvents.push("end");
            }}
            onPaste={(e) => void handlePasteEvent(e.nativeEvent)}
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
            className="block w-full min-h-5 flex-1 self-center resize-none bg-transparent py-0 pr-1 text-[15px] leading-5 outline-none placeholder:text-[var(--ct-muted-fg)]"
            style={{
              color: "var(--ct-fg, #c9d1d9)",
              border: "none",
              lineHeight: `${COMPOSER_MIN_HEIGHT}px`,
              margin: 0,
              overflowY: "auto",
            }}
          />
        </ComposerActions>
      </div>
    </div>
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
  && a.modelBubble === b.modelBubble
  && a.chatBackend === b.chatBackend
  && a.isActive === b.isActive
);
