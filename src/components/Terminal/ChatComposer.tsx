import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Clock3, Paperclip, Terminal, X } from "lucide-react";
import { ComposerActions } from "./ComposerActions";
import { invoke, listen } from "../../lib/bridge";
import { quotePathForPaste } from "../../lib/clipboardPaste";
import { usePersistentDraft } from "../../lib/usePersistentDraft";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import {
  createDelayedMessage,
  deleteDelayedMessage,
  listDelayedMessages,
  sendDelayedMessageNow,
  updateDelayedMessage,
  type DelayedMessage,
} from "./delayedSend";
import { DelayedSendQueuePanel, DelayedSendSchedulePanel } from "./DelayedSendPanel";
import {
  filterSlashCommands,
  type SlashCommandSuggestion,
} from "../../lib/slashCommands";
import type { ModelInfo, PendingPrompt, ProviderDescriptor } from "../../lib/types";
import { ModelSuggestionPicker } from "./ModelSuggestionPicker";

const COMPOSER_MIN_HEIGHT = 20;
const COMPOSER_SINGLE_LINE_THRESHOLD = 28;
const COMPOSER_MAX_HEIGHT = 230;
const AUDIO_ATTACHMENT_RE = /\.(webm|ogg|oga|opus|mp3|wav|m4a|aac|mp4|flac)$/i;
// Shown the instant a voice note is sent — transcription (cold Whisper can take
// several seconds) then morphs this bubble into the transcript, or an error.
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

function composerHeight(el: HTMLTextAreaElement, value: string): number {
  const hasExplicitLineBreak = value.includes("\n");
  if (!hasExplicitLineBreak && el.scrollHeight <= COMPOSER_SINGLE_LINE_THRESHOLD) {
    return COMPOSER_MIN_HEIGHT;
  }
  return Math.min(Math.max(el.scrollHeight, COMPOSER_MIN_HEIGHT), COMPOSER_MAX_HEIGHT);
}

function cliFallbackCommandPayload(text: string): string | null {
  const trimmed = text.trim();
  if (!CLI_FALLBACK_COMMANDS.has(trimmed)) return null;
  return `${trimmed}\r`;
}

function formatModelSwitch(format: string, modelId: string): string {
  return format.includes("{model}")
    ? format.replaceAll("{model}", modelId)
    : `${format} ${modelId}`;
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
  onToggleTerminalFallback,
  terminalActive,
  pendingPrompt,
  queuedCount,
  failedCount,
  onOpenDelayedQueue,
}: {
  attachments: string[];
  onRemove: (path: string) => void;
  onToggleTerminalFallback?: () => void;
  terminalActive: boolean;
  pendingPrompt?: PendingPrompt | null;
  queuedCount: number;
  failedCount: number;
  onOpenDelayedQueue: () => void;
}) {
  if (attachments.length === 0 && !onToggleTerminalFallback && queuedCount === 0 && failedCount === 0) return null;
  const visible = attachments.slice(0, 4);
  const overflow = attachments.length - visible.length;
  return (
    <div
      data-testid="composer-bubble-row"
      className="mb-2 flex max-h-[3.6rem] flex-wrap items-center gap-1 overflow-hidden"
    >
      {onToggleTerminalFallback && (
        <button
          type="button"
          data-testid="terminal-fallback-bubble"
          data-terminal-fallback-toggle="true"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => onToggleTerminalFallback()}
          className="inline-flex h-7 items-center gap-1 rounded-full border px-2 text-xs font-mono hover:brightness-110"
          style={{
            background: "color-mix(in srgb, var(--ct-bg, #0d1117) 88%, white 5%)",
            borderColor: "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 28%, transparent)",
            color: "var(--ct-muted-fg)",
          }}
          title="Toggle terminal fallback"
        >
          <Terminal size={12} />
          <span>terminal</span>
          {terminalActive && !pendingPrompt && (
            <span
              className="h-1.5 w-1.5 rounded-full"
              style={{ background: "var(--ct-green, #3fb950)" }}
            />
          )}
          {pendingPrompt && (
            <span
              className="ml-1 rounded px-1 py-0.5 text-[10px]"
              style={{
                background: "color-mix(in srgb, var(--ct-yellow, #d29922) 18%, transparent)",
                color: "var(--ct-yellow, #d29922)",
              }}
            >
              waiting
            </span>
          )}
        </button>
      )}
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

export function ChatComposer({
  paneId,
  paneProvider,
  currentPtyId,
  pendingPrompt,
  terminalHasAttention,
  attachments,
  addAttachments,
  removeAttachment,
  clearAttachments,
  handlePasteEvent,
  addOptimistic,
  updateOptimistic,
  showComposerError,
  onOpenTerminalFallback,
  onToggleTerminalFallback,
  chatBackend = false,
}: {
  paneId: string;
  paneProvider: string | null;
  currentPtyId: string | null;
  /** This pane is backed by a plugin chatBackend (no PTY) — send via the
   *  pane-level `chat_send_text` instead of writing to a PTY. */
  chatBackend?: boolean;
  pendingPrompt: PendingPrompt | null;
  terminalHasAttention: boolean;
  attachments: string[];
  addAttachments: (paths: string[]) => void;
  removeAttachment: (path: string) => void;
  clearAttachments: () => void;
  handlePasteEvent: (event: ClipboardEvent) => void | Promise<void>;
  addOptimistic: (content: string) => string;
  updateOptimistic: (id: string, content: string) => void;
  showComposerError: (message: string) => void;
  onOpenTerminalFallback?: () => void;
  onToggleTerminalFallback?: () => void;
}) {
  const [input, setInput] = usePersistentDraft(`pane-session-chat:${paneId}`);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerHighlight, setPickerHighlight] = useState(0);
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const [providerDescriptor, setProviderDescriptor] = useState<ProviderDescriptor | null>(null);
  const [modelOptions, setModelOptions] = useState<ModelInfo[]>([]);
  const [modelHighlight, setModelHighlight] = useState(0);
  const touchMode = useTouchOptimizedUi();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const lastSendRef = useRef<{ text: string; at: number }>({ text: "", at: 0 });
  const [delayedMessages, setDelayedMessages] = useState<DelayedMessage[]>([]);
  const [delayedQueueOpen, setDelayedQueueOpen] = useState(false);
  const [scheduleOpen, setScheduleOpen] = useState(false);
  const [delayedBusyId, setDelayedBusyId] = useState<string | null>(null);
  const [scheduleBusy, setScheduleBusy] = useState(false);
  const [modelSuggestionDismissedFor, setModelSuggestionDismissedFor] = useState<string | null>(null);
  const firstLine = input.split("\n")[0] || "";
  const modelCommandQuery = useMemo(() => {
    const match = firstLine.match(/^\/model\s+([^\n]*)$/i);
    return match ? match[1].trim().toLowerCase() : null;
  }, [firstLine]);
  const filteredModelOptions = useMemo(() => {
    if (modelCommandQuery === null) return [];
    if (!providerDescriptor?.model_switch_format) return [];
    const needle = modelCommandQuery;
    if (!needle) return modelOptions;
    return modelOptions.filter((model) => {
      const haystack = `${model.id} ${model.display_name} ${model.description ?? ""}`.toLowerCase();
      return haystack.includes(needle);
    });
  }, [modelCommandQuery, modelOptions, providerDescriptor?.model_switch_format]);
  const modelSuggestionOpen = modelCommandQuery !== null
    && filteredModelOptions.length > 0
    && !modelPickerOpen
    && modelSuggestionDismissedFor !== firstLine;

  const suggestions = useMemo(
    () => filterSlashCommands(firstLine),
    [firstLine],
  );

  const refocusComposer = useCallback(() => {
    const focus = () => textareaRef.current?.focus({ preventScroll: true });
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

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const next = composerHeight(el, input);
    el.style.height = `${next}px`;
  }, [input]);

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
    let cancelled = false;
    invoke<ProviderDescriptor[]>("list_agent_providers")
      .then((providers) => {
        if (cancelled) return;
        const provider = providers.find((candidate) => candidate.id === paneProvider) ?? null;
        setProviderDescriptor(provider);
        const models = provider?.available_models ?? [];
        setModelOptions(models);
        if (models.length) {
          const defaultIndex = provider?.default_model
            ? models.findIndex((model) => model.id === provider.default_model)
            : -1;
          setModelHighlight(defaultIndex >= 0 ? defaultIndex : 0);
        } else {
          setModelHighlight(0);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [paneProvider]);

  useEffect(() => {
    const id = setTimeout(() => textareaRef.current?.focus(), 0);
    return () => clearTimeout(id);
  }, []);

  const refreshDelayedMessages = useCallback(async () => {
    try {
      setDelayedMessages(await listDelayedMessages(paneId));
    } catch {
      setDelayedMessages([]);
    }
  }, [paneId]);

  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      try {
        const messages = await listDelayedMessages(paneId);
        if (!cancelled) setDelayedMessages(messages);
      } catch {
        if (!cancelled) setDelayedMessages([]);
      }
    };
    void refresh();
    const interval = window.setInterval(refresh, 3_000);
    let unlisten: (() => void) | null = null;
    void listen<{ paneId?: string }>("delayed-send-changed", (e) => {
      if (!e.payload?.paneId || e.payload.paneId === paneId) void refresh();
    }).then((fn) => {
      unlisten = fn;
    }).catch(() => {});
    return () => {
      cancelled = true;
      window.clearInterval(interval);
      unlisten?.();
    };
  }, [paneId]);

  function pickSuggestion(cmd: SlashCommandSuggestion) {
    const lines = input.split("\n");
    lines[0] = cmd.name + " ";
    setInput(lines.join("\n"));
    setPickerOpen(false);
    refocusComposer();
  }

  const sendRawToPty = useCallback(
    async (data: string) => {
      if (!currentPtyId) throw new Error("Pane has no PTY");
      await invoke("write_pty_string", { ptyId: currentPtyId, data, raw: false });
    },
    [currentPtyId],
  );

  const chooseModel = useCallback(
    async (model: ModelInfo) => {
      setModelPickerOpen(false);
      setInput("");
      try {
        const format = providerDescriptor?.model_switch_format;
        if (!format) {
          throw new Error(
            providerDescriptor
              ? `${providerDescriptor.display_name} does not support live model switching.`
              : "This provider does not support live model switching.",
          );
        }
        await sendRawToPty(`${formatModelSwitch(format, model.id)}\r`);
      } catch (e) {
        showComposerError(e instanceof Error ? e.message : String(e));
      } finally {
        refocusComposer();
      }
    },
    [providerDescriptor, refocusComposer, sendRawToPty, setInput, showComposerError],
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
    return { text, allAttachments };
  }

  async function composeDraft(text: string, allAttachments: string[]) {
    let transcribeFailed = false;
    const pathParts = await Promise.all(
      allAttachments.map(async (p) => {
        if (!AUDIO_ATTACHMENT_RE.test(p)) return quotePathForPaste(p);
        try {
          return await invoke<string>("transcribe_voice_attachment", { path: p });
        } catch {
          transcribeFailed = true;
          return quotePathForPaste(p);
        }
      }),
    );
    const pathLines = pathParts.join(" ");
    const composed = text && pathLines
      ? `${text} ${pathLines}`
      : text || pathLines;
    return { composed, transcribeFailed };
  }

  async function send(extraAttachments: string[] = []) {
    const draft = buildDraft(extraAttachments);
    if (!draft) return;
    const { text, allAttachments } = draft;
    const nowTs = Date.now();
    const dupKey = `${text} ${allAttachments.join("|")}`;
    if (dupKey === lastSendRef.current.text && nowTs - lastSendRef.current.at < 500) return;
    lastSendRef.current = { text: dupKey, at: nowTs };
    // Voice notes: show the bubble immediately so the user sees progress during
    // the (possibly multi-second, cold-Whisper) transcription gap, then morph it
    // into the transcript — instead of staring at silence until the text lands.
    const hasAudio = allAttachments.some((p) => AUDIO_ATTACHMENT_RE.test(p));
    const placeholderId = hasAudio ? addOptimistic(VOICE_TRANSCRIBING_PLACEHOLDER) : null;
    const { composed, transcribeFailed } = await composeDraft(text, allAttachments);
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
      && providerDescriptor?.model_switch_format
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
      if (modelOptions.length === 0 || !providerDescriptor?.model_switch_format) {
        showComposerError(
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
    if (placeholderId) {
      updateOptimistic(placeholderId, composed);
    } else {
      addOptimistic(composed);
    }
    setInput("");
    clearAttachments();
    refocusComposer();
    try {
      if (chatBackend) {
        // No PTY — route to the pane send seam, which dispatches to the
        // plugin's sendMessage (chat_send_text → send_text_when_ready).
        await invoke("chat_send_text", { paneId, text: composed, enter: true });
      } else {
        const fallbackCommand = allAttachments.length === 0 ? cliFallbackCommandPayload(composed) : null;
        if (!currentPtyId) throw new Error("Pane has no PTY");
        if (fallbackCommand) {
          onOpenTerminalFallback?.();
          await invoke("write_pty_string", { ptyId: currentPtyId, data: fallbackCommand, raw: false });
        } else {
          await invoke("write_pty_string", { ptyId: currentPtyId, data: composed + "\r", raw: false });
        }
      }
    } catch (e) {
      showComposerError(e instanceof Error ? e.message : String(e));
    } finally {
      refocusComposer();
    }
  }

  async function scheduleDraft(scheduledAtMs: number | null) {
    const draft = buildDraft();
    if (!draft) return;
    setScheduleBusy(true);
    try {
      const { composed, transcribeFailed } = await composeDraft(draft.text, draft.allAttachments);
      if (transcribeFailed) {
        showComposerError(VOICE_TRANSCRIBE_ERROR);
        return;
      }
      if (draft.allAttachments.length === 0 && composed.trim() === "/model") {
        showComposerError("Model picker cannot be scheduled.");
        return;
      }
      await createDelayedMessage(paneId, composed, scheduledAtMs);
      setInput("");
      clearAttachments();
      setScheduleOpen(false);
      setDelayedQueueOpen(true);
      await refreshDelayedMessages();
    } catch (e) {
      showComposerError(e instanceof Error ? e.message : String(e));
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
      className="relative shrink-0 p-2 border-t"
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
              onConfirm={(scheduledAtMs) => void scheduleDraft(scheduledAtMs)}
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
          <div className="grid gap-1">
            {modelOptions.map((model, i) => {
              const selected = i === modelHighlight;
              return (
                <button
                  key={model.id}
                  type="button"
                  onMouseDown={(e) => e.preventDefault()}
                  onMouseEnter={() => setModelHighlight(i)}
                  onClick={() => void chooseModel(model)}
                  className="w-full grid grid-cols-[1rem_minmax(0,1fr)] gap-2 rounded px-2 py-1.5 text-left text-xs"
                  style={{
                    background: selected
                      ? "color-mix(in srgb, var(--ct-accent, #58a6ff) 18%, transparent)"
                      : "transparent",
                    color: selected ? "var(--ct-fg)" : "var(--ct-muted-fg)",
                  }}
                >
                  <span className="text-center font-mono">{selected ? ">" : ""}</span>
                  <span className="min-w-0">
                    <span className="block truncate font-medium">{model.display_name}</span>
                    <span className="block truncate opacity-60 font-mono">{model.id}</span>
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      )}
      <ComposerBubbleRow
        attachments={attachments}
        onRemove={removeAttachment}
        onToggleTerminalFallback={onToggleTerminalFallback}
        terminalActive={terminalHasAttention}
        pendingPrompt={pendingPrompt}
        queuedCount={queuedCount}
        failedCount={failedCount}
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
          onError={showComposerError}
          sendDisabled={!input.trim() && attachments.length === 0}
          hasText={input.trim().length > 0 || attachments.length > 0}
        >
          <SlashPicker
            suggestions={pickerOpen ? suggestions : []}
            highlighted={pickerHighlight}
            onPick={pickSuggestion}
          />
          <ModelSuggestionPicker
            models={modelSuggestionOpen ? filteredModelOptions : []}
            highlighted={modelHighlight}
            defaultModel={providerDescriptor?.default_model}
            onHighlight={setModelHighlight}
            onPick={(model) => void chooseModel(model)}
          />
          <textarea
            data-testid="chat-composer"
            ref={textareaRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
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
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void send();
              }
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
