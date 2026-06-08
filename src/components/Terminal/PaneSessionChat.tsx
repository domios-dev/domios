import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { listen, type UnlistenFn } from "../../lib/bridge";
import { ComposerActions, useWebFileDrop } from "./ComposerActions";
import { Paperclip, Terminal, X } from "lucide-react";
import { MessageBubble } from "@codeterm/chat-ui";
import { AgentIcon } from "../AgentIcon";
import { getPlugin, useRegistryVersion } from "../../lib/plugins/registry";
import {
  dedupeRecentUserMessages,
  matchesRecentUserMessage,
  type NormalizedChatMessage as ChatMessage,
} from "@codeterm/chat-core";
import { detectChatPrefix, splitChatPrefixedMessages, type ChatOrigin } from "../../lib/chatPrefixes";
import { useMultiplexer } from "../../store/multiplexer";
import { quotePathForPaste } from "../../lib/clipboardPaste";
import { usePersistentDraft } from "../../lib/usePersistentDraft";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import { useTauriFileDrop } from "../../hooks/terminal";
import {
  filterSlashCommands,
  type SlashCommandSuggestion,
} from "../../lib/slashCommands";
import type { ModelInfo, ProviderDescriptor } from "../../lib/types";
import type { PendingPrompt } from "../../lib/types";

// Optimistic messages auto-expire by TTL when JSONL never echoes them back.
const OPTIMISTIC_TTL_MS = 60_000;
const INITIAL_VISIBLE_MESSAGES = 220;
const VISIBLE_MESSAGE_STEP = 160;
const COMPOSER_MIN_HEIGHT = 20;
const COMPOSER_SINGLE_LINE_THRESHOLD = 28;
const COMPOSER_MAX_HEIGHT = 150;
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

function isMicrophonePermissionError(message: string): boolean {
  const normalized = message.toLowerCase();
  if (normalized.includes("needs https")) return false;
  return (
    normalized.includes("microphone") ||
    normalized.includes("mic api") ||
    normalized.includes("media device") ||
    normalized.includes("getusermedia") ||
    normalized.includes("notallowederror") ||
    normalized.includes("denied permission") ||
    normalized.includes("permission denied") ||
    normalized.includes("not allowed by the user agent")
  );
}

interface ChatStartResult {
  text: string;
  offset: number;
  start_offset?: number;
  pending?: boolean;
  // Structured providers (sqlite-backed, e.g. opencode) return parsed messages
  // here instead of raw `text`; deltas arrive on `pane-chat-msg-{id}`.
  messages?: ChatMessage[];
  cursor?: string;
}

interface RawDeltaEvent {
  pane_id: string;
  text: string;
}

interface ChatMsgEvent {
  pane_id: string;
  messages: ChatMessage[];
  cursor?: string;
}

// Merge structured-chat deltas by id (replace-or-append), ordered by the
// numeric ms timestamp so a re-read of the growing tail message stays in place.
function mergeById(prev: ChatMessage[], incoming: ChatMessage[]): ChatMessage[] {
  const map = new Map<string, ChatMessage>();
  for (const m of prev) map.set(m.id, m);
  for (const m of incoming) map.set(m.id, m);
  return Array.from(map.values()).sort(
    (a, b) => Number(a.timestamp ?? 0) - Number(b.timestamp ?? 0),
  );
}

interface Props {
  paneId: string;
  onError?: (msg: string) => void;
  onOpenTerminalFallback?: () => void;
  onToggleTerminalFallback?: () => void;
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
}: {
  attachments: string[];
  onRemove: (path: string) => void;
  onToggleTerminalFallback?: () => void;
  terminalActive: boolean;
  pendingPrompt?: PendingPrompt | null;
}) {
  if (attachments.length === 0 && !onToggleTerminalFallback) return null;
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

function OriginProviderAvatar({ providerId }: { providerId: string | null | undefined }) {
  if (!providerId) return null;
  return (
    <span
      className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-full border"
      style={{
        background: "color-mix(in srgb, var(--ct-surface, #161b22) 86%, var(--ct-accent, #58a6ff) 14%)",
        borderColor: "color-mix(in srgb, var(--ct-accent, #58a6ff) 28%, transparent)",
        color: "var(--ct-fg)",
      }}
      aria-hidden
    >
      <AgentIcon providerId={providerId} className="text-[15px]" />
    </span>
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

const AUDIO_ATTACHMENT_RE = /\.(webm|ogg|oga|opus|mp3|wav|m4a|aac|mp4|flac)$/i;

function cliFallbackCommandPayload(text: string): string | null {
  const trimmed = text.trim();
  if (!CLI_FALLBACK_COMMANDS.has(trimmed)) return null;
  return `${trimmed}\r`;
}

function groupToolResults(messages: ChatMessage[]): ChatMessage[] {
  const grouped: ChatMessage[] = [];
  const toolIndex = new Map<string, number>();
  for (const message of messages) {
    if (message.type === "tool_call" && message.toolId) {
      toolIndex.set(message.toolId, grouped.length);
      grouped.push(message);
      continue;
    }
    if (message.type === "tool_result" && message.toolId) {
      const index = toolIndex.get(message.toolId);
      if (index != null) {
        const existing = grouped[index];
        const toolResult = existing.toolResult
          ? `${existing.toolResult}\n${message.content}`
          : message.content;
        grouped[index] = { ...existing, toolResult };
        continue;
      }
    }
    grouped.push(message);
  }
  return grouped;
}

function formatModelSwitch(format: string, modelId: string): string {
  return format.includes("{model}")
    ? format.replaceAll("{model}", modelId)
    : `${format} ${modelId}`;
}

export function PaneSessionChat({
  paneId,
  onError,
  onOpenTerminalFallback,
  onToggleTerminalFallback,
}: Props) {
  const [rawText, setRawText] = useState<string>("");
  // Structured (DB-backed) providers stream parsed messages keyed by id; null
  // means JSONL mode (browser parses rawText).
  const [structured, setStructured] = useState<ChatMessage[] | null>(null);
  const [earliestOffset, setEarliestOffset] = useState(0);
  const [visibleMessageCount, setVisibleMessageCount] = useState(INITIAL_VISIBLE_MESSAGES);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [input, setInput] = usePersistentDraft(`pane-session-chat:${paneId}`);
  const [optimistic, setOptimistic] = useState<ChatMessage[]>([]);
  const [attachments, setAttachments] = useState<string[]>([]);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickerHighlight, setPickerHighlight] = useState(0);
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const [providerDescriptor, setProviderDescriptor] = useState<ProviderDescriptor | null>(null);
  const [modelOptions, setModelOptions] = useState<ModelInfo[]>([]);
  const [modelHighlight, setModelHighlight] = useState(0);
  const [terminalActive, setTerminalActive] = useState(false);
  const touchMode = useTouchOptimizedUi();
  const scrollRef = useRef<HTMLDivElement>(null);
  const atBottomRef = useRef(true);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const terminalTextRef = useRef<string | null>(null);
  const terminalActiveTimerRef = useRef<number | null>(null);
  // Dedup only the accidental keyboard+button double-fire of ONE action
  // (same composed text within a blink) — deliberate rapid sends must pass.
  const lastSendRef = useRef<{ text: string; at: number }>({ text: "", at: 0 });
  const onErrorRef = useRef(onError);
  // chatRootRef scopes the drag-drop zone to the entire chat pane.
  const chatRootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    onErrorRef.current = onError;
  }, [onError]);

  const suggestions = useMemo(
    () => filterSlashCommands(input.split("\n")[0] || ""),
    [input],
  );
  const { state: muxState, setFocusedPane, addNotification } = useMultiplexer();
  const paneProvider = muxState.panes[paneId]?.agent_type ?? null;

  const showComposerError = useCallback((message: string) => {
    addNotification({
      pane_id: paneId,
      workspace_id: null,
      title: isMicrophonePermissionError(message) ? "Microphone unavailable" : "Chat error",
      body: isMicrophonePermissionError(message)
        ? `${message} Open microphone permissions in Settings if needed.`
        : message,
      level: "Error",
    });
  }, [addNotification, paneId]);

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

  // Open picker when first line starts with / and there are matches.
  useEffect(() => {
    const firstLine = input.split("\n")[0] || "";
    setPickerOpen(!modelPickerOpen && firstLine.startsWith("/") && suggestions.length > 0);
    setPickerHighlight(0);
  }, [input, modelPickerOpen, suggestions.length]);

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

  // Append paths to the attachment list, de-duping.
  const addAttachments = useCallback((paths: string[]) => {
    const cleanPaths = paths
      .map((p) => p.trim())
      .filter((p, i, all) => p.length > 0 && all.indexOf(p) === i);
    if (cleanPaths.length === 0) return;
    setAttachments((prev) => {
      const seen = new Set(prev);
      const next = [...prev];
      for (const p of cleanPaths) {
        if (!seen.has(p)) {
          seen.add(p);
          next.push(p);
        }
      }
      return next;
    });
  }, []);

  // A fresh agent pane reports pending=true (no JSONL session yet) — render
  // the normal empty chat (sends ride the PTY) and re-run this effect when
  // the mux snapshot delivers the session id.
  const agentSessionId = muxState.panes[paneId]?.agent_session_id ?? null;

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;
    (async () => {
      try {
        const res = await invoke<ChatStartResult>("chat_start", { paneId });
        if (cancelled) return;
        setError(null);
        setRawText(res.text || "");
        setEarliestOffset(res.start_offset ?? 0);
        setVisibleMessageCount(INITIAL_VISIBLE_MESSAGES);
        setLoading(false);
        if (res.pending) return;
        if (res.messages) {
          // Structured (DB-backed) provider: render parsed messages directly,
          // merge deltas by id (an in-progress message updates in place).
          setStructured(res.messages);
          unlisten = await listen<ChatMsgEvent>(
            `pane-chat-msg-${paneId}`,
            (ev) => {
              if (cancelled) return;
              const incoming = ev.payload.messages;
              if (!incoming?.length) return;
              setStructured((prev) => mergeById(prev ?? [], incoming));
            },
          );
          return;
        }
        setStructured(null);
        unlisten = await listen<RawDeltaEvent>(
          `pane-chat-raw-${paneId}`,
          (ev) => {
            if (cancelled) return;
            const newText = ev.payload.text || "";
            if (!newText) return;
            setRawText((prev) => prev + newText);
          },
        );
      } catch (e) {
        if (cancelled) return;
        const msg = e instanceof Error ? e.message : String(e);
        setError(msg);
        setLoading(false);
        onErrorRef.current?.(msg);
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
      void invoke("chat_stop", { paneId }).catch(() => {});
    };
  }, [paneId, agentSessionId]);

  useEffect(() => {
    if (error?.includes("chat_send_raw")) setError(null);
  }, [error]);

  // Focus the composer when chat opens.
  useEffect(() => {
    const id = setTimeout(() => textareaRef.current?.focus(), 0);
    return () => clearTimeout(id);
  }, []);

  // Re-parse when a plugin (un)registers — HMR swaps a provider's parser.
  const registryVersion = useRegistryVersion();
  const parsed = useMemo<ChatMessage[]>(() => {
    if (structured) return structured;
    return getPlugin(paneProvider)?.parseSession?.(rawText) ?? [];
  }, [structured, rawText, paneProvider, registryVersion]);

  // Drop optimistic entries once equivalent parsed user input appears in JSONL.
  // Match uses normalized text plus attachment fingerprint.
  const matchesParsedUser = useCallback(
    (optimisticMessage: ChatMessage, recentParsed: ChatMessage[]) => {
      return matchesRecentUserMessage(optimisticMessage, recentParsed);
    },
    [],
  );

  const allMessages = useMemo(() => {
    const dedupedParsed = dedupeRecentUserMessages(parsed);
    const groupedParsed = groupToolResults(dedupedParsed);
    if (optimistic.length === 0) return groupedParsed;
    const tail = Math.max(optimistic.length * 4, 20);
    const recentParsed = groupedParsed.slice(-tail);
    const stillPending = optimistic.filter(
      (m) => !matchesParsedUser(m, recentParsed),
    );
    return [...groupedParsed, ...stillPending];
  }, [parsed, optimistic, matchesParsedUser]);

  const currentPtyId = muxState.panes[paneId]?.pty_id ?? null;
  const pendingPrompt: PendingPrompt | null = muxState.panes[paneId]?.pending_prompt ?? null;
  const terminalHasAttention = terminalActive || Boolean(pendingPrompt);

  const enrichedMessages = useMemo(() => {
    const visible = allMessages.slice(-visibleMessageCount);
    const out: Array<{ msg: ChatMessage; origin: ChatOrigin | undefined }> = [];
    for (const m of visible) {
      if (m.type !== "user") {
        out.push({ msg: m, origin: undefined });
        continue;
      }
      const splitPrefixes = splitChatPrefixedMessages(m.content);
      if (splitPrefixes && splitPrefixes.length > 1) {
        splitPrefixes.forEach((prefix, index) => {
          out.push({
            msg: { ...m, id: `${m.id}-prefix-${index}`, content: prefix.body },
            origin: prefix.origin,
          });
        });
        continue;
      }
      const prefix = detectChatPrefix(m.content);
      if (!prefix) {
        out.push({ msg: m, origin: undefined });
        continue;
      }
      out.push({
        msg: { ...m, content: prefix.body },
        origin: prefix.origin,
      });
    }
    return out;
  }, [allMessages, visibleMessageCount]);

  const hiddenMessageCount = Math.max(0, allMessages.length - enrichedMessages.length);

  const loadOlder = useCallback(async () => {
    if (loadingOlder) return;
    if (hiddenMessageCount > 0) {
      setVisibleMessageCount((count) => count + VISIBLE_MESSAGE_STEP);
      return;
    }
    if (earliestOffset === 0) return;
    setLoadingOlder(true);
    try {
      const res = await invoke<{ text: string; start_offset: number; end_offset: number }>(
        "chat_backfill",
        { paneId, beforeOffset: earliestOffset, maxBytes: 256 * 1024 },
      );
      if (res.text) {
        setRawText((prev) => `${res.text}${prev}`);
        setEarliestOffset(res.start_offset);
        setVisibleMessageCount((count) => count + VISIBLE_MESSAGE_STEP);
      } else {
        setEarliestOffset(res.start_offset);
      }
    } catch (e) {
      showComposerError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoadingOlder(false);
    }
  }, [earliestOffset, hiddenMessageCount, loadingOlder, paneId]);

  useEffect(() => {
    if (!currentPtyId) {
      terminalTextRef.current = null;
      setTerminalActive(false);
      return;
    }
    let cancelled = false;
    const poll = async () => {
      try {
        const text = await invoke<string>("get_pane_text", { paneId, formatted: false });
        if (cancelled) return;
        const previous = terminalTextRef.current;
        terminalTextRef.current = text;
        if (previous !== null && previous !== text) {
          setTerminalActive(true);
          if (terminalActiveTimerRef.current) {
            window.clearTimeout(terminalActiveTimerRef.current);
          }
          terminalActiveTimerRef.current = window.setTimeout(() => {
            setTerminalActive(false);
            terminalActiveTimerRef.current = null;
          }, 3_000);
        }
      } catch {
        if (!cancelled) setTerminalActive(false);
      }
    };
    void poll();
    const id = window.setInterval(() => void poll(), 1_500);
    return () => {
      cancelled = true;
      window.clearInterval(id);
      if (terminalActiveTimerRef.current) {
        window.clearTimeout(terminalActiveTimerRef.current);
        terminalActiveTimerRef.current = null;
      }
    };
  }, [currentPtyId, paneId]);

  // Garbage-collect optimistic items as soon as JSONL catches up.
  // Also clear stale entries after TTL to avoid forever-pinned bubbles.
  useEffect(() => {
    if (optimistic.length === 0) return;
    const now = Date.now();
    const tail = Math.max(optimistic.length * 4, 20);
    const recentParsed = parsed.slice(-tail);
    setOptimistic((prev) => {
      const next = prev.filter((m) => {
        if (matchesParsedUser(m, recentParsed)) return false;
        if (m.timestamp) {
          const sentAt = Number(m.timestamp);
          if (!Number.isNaN(sentAt) && now - sentAt > OPTIMISTIC_TTL_MS) {
            return false;
          }
        }
        return true;
      });
      return next.length === prev.length ? prev : next;
    });
  }, [parsed, optimistic.length, matchesParsedUser]);

  useEffect(() => {
    if (!scrollRef.current) return;
    if (atBottomRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [allMessages]);

  // Poll TTL cleanup every 10s so stale optimistic messages clear even without new JSONL.
  useEffect(() => {
    if (optimistic.length === 0) return;
    const id = setInterval(() => {
      const now = Date.now();
      setOptimistic((prev) => {
        const next = prev.filter((m) => {
          if (!m.timestamp) return true;
          const sentAt = Number(m.timestamp);
          if (Number.isNaN(sentAt)) return true;
          return now - sentAt <= OPTIMISTIC_TTL_MS;
        });
        return next.length === prev.length ? prev : next;
      });
    }, 10_000);
    return () => clearInterval(id);
  }, [optimistic.length]);

  // Bug 2: drag-drop scoped to the entire chat pane (chatRootRef).
  useTauriFileDrop({
    containerRef: chatRootRef as React.RefObject<HTMLElement | null>,
    enabled: !loading && !error,
    onPaths: (paths) => addAttachments(paths),
  });

  // Web drag-drop uploads to daemon, then attaches server paths; Tauri uses native paths.
  const { dragOver: webDragOver, handlers: webDropHandlers } = useWebFileDrop(
    addAttachments,
    showComposerError,
  );

  async function send(extraAttachments: string[] = []) {
    const text = input.trim();
    const cleanExtraAttachments = extraAttachments
      .map((p) => p.trim())
      .filter((p, i, all) => p.length > 0 && all.indexOf(p) === i);
    const allAttachments = [...attachments];
    for (const p of cleanExtraAttachments) {
      if (!allAttachments.includes(p)) allAttachments.push(p);
    }
    if (!text && allAttachments.length === 0) return;
    // No in-flight lock: the daemon serializes PTY writes and the agent
    // queues input, so you can fire as many messages as you like back to
    // back. We only drop an exact-duplicate within 500ms (the Enter-key +
    // send-button double-fire of a single action).
    const nowTs = Date.now();
    const dupKey = `${text} ${allAttachments.join("|")}`;
    if (dupKey === lastSendRef.current.text && nowTs - lastSendRef.current.at < 500) return;
    lastSendRef.current = { text: dupKey, at: nowTs };
    // Compose text + attachments. Audio is wrapped as `[voice] (transcript) file://`
    // (transcribed when a daemon is configured) — same shape as the Telegram
    // bridge, and the `[...]` prefix avoids a leading `/` that opencode's TUI
    // treats as a command. Non-audio keeps the quoted-path form.
    const pathParts = await Promise.all(
      allAttachments.map(async (p) =>
        AUDIO_ATTACHMENT_RE.test(p)
          ? await invoke<string>("transcribe_voice_attachment", { path: p }).catch(
              () => quotePathForPaste(p),
            )
          : quotePathForPaste(p),
      ),
    );
    const pathLines = pathParts.join(" ");
    const composed = text && pathLines
      ? `${text} ${pathLines}`
      : text || pathLines;
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
    setOptimistic((prev) => [
      ...prev,
      {
        id: `opt-${Date.now()}`,
        type: "user",
        content: composed,
        // Store optimistic send time as a stringified millis value for TTL pruning.
        timestamp: String(Date.now()),
      },
    ]);
    // Invariant: sending clears the draft but focus MUST stay in the composer —
    // the mobile keyboard must not dismiss and desktop must not need a re-click.
    setInput("");
    setAttachments([]);
    refocusComposer();
    try {
      const fallbackCommand = allAttachments.length === 0 ? cliFallbackCommandPayload(composed) : null;
      if (fallbackCommand) {
        if (!currentPtyId) throw new Error("Pane has no PTY");
        onOpenTerminalFallback?.();
        await invoke("write_pty_string", { ptyId: currentPtyId, data: fallbackCommand });
      } else {
        await invoke("chat_send_text", { paneId, text: composed, enter: true });
      }
    } catch (e) {
      showComposerError(e instanceof Error ? e.message : String(e));
    } finally {
      refocusComposer();
    }
  }

  // Shared paste handler for textarea + window listener (plain ClipboardEvent, not React's).
  const handlePasteEvent = useCallback(async (e: ClipboardEvent) => {
    const cd = e.clipboardData;
    if (!cd) return;

    // 1) OS-native file refs via Tauri (Explorer/Finder, NSPasteboardTypeFileURL).
    try {
      const paths = await invoke<string[]>("read_clipboard_files");
      if (paths && paths.length > 0) {
        e.preventDefault();
        addAttachments(paths);
        return;
      }
    } catch { /* fall through to image / text */ }

    // 2. Image data (screenshots, "copy image" from browser).
    const imageItem = Array.from(cd.items).find(
      (it) => it.kind === "file" && it.type.startsWith("image/"),
    );
    if (imageItem) {
      // Prevent default synchronously before any async work.
      e.preventDefault();
      const file = imageItem.getAsFile();
      if (!file) return;
      const reader = new FileReader();
      reader.onloadend = async () => {
        const result = reader.result;
        if (typeof result !== "string") return;
        // result = "data:image/png;base64,iVBOR..." — strip the data: prefix.
        const commaIdx = result.indexOf(",");
        const b64 = commaIdx >= 0 ? result.slice(commaIdx + 1) : result;
        const mime = file.type || "image/png";
        const suffix = mime.split("/")[1] || "png";
        try {
          const path = await invoke<string>("save_temp_file", {
            data: b64,
            extension: suffix,
          });
          addAttachments([path]);
        } catch (err) {
          console.warn("[chat] paste-image failed:", err);
        }
      };
      reader.readAsDataURL(file);
      return;
    }

    // 3. Otherwise — let the textarea's default paste behaviour handle text.
  }, [addAttachments]);

  // Window paste listener for Ctrl+V without focus; only active inside chat root.
  useEffect(() => {
    const onWindowPaste = (e: ClipboardEvent) => {
      if (
        chatRootRef.current &&
        !chatRootRef.current.contains(document.activeElement)
      ) {
        return;
      }
      void handlePasteEvent(e);
    };
    window.addEventListener("paste", onWindowPaste);
    return () => window.removeEventListener("paste", onWindowPaste);
  }, [handlePasteEvent]);

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
      await invoke("write_pty_string", { ptyId: currentPtyId, data });
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
    [providerDescriptor, refocusComposer, sendRawToPty, showComposerError],
  );

  if (error) {
    return (
      <div
        className="w-full h-full flex items-center justify-center p-4 text-sm"
        style={{ color: "var(--ct-muted-fg, #8b949e)" }}
      >
        <div className="text-center">
          <div className="mb-2">UI mode unavailable for this pane.</div>
          <div className="text-xs opacity-60">{error}</div>
        </div>
      </div>
    );
  }

  if (loading) {
    return <div className="p-4 text-sm opacity-60">Loading chat…</div>;
  }

  return (
    <div
      ref={chatRootRef}
      className="w-full h-full flex flex-col relative"
      style={{
        background: "var(--ct-bg, #0d1117)",
        color: "var(--ct-fg, #c9d1d9)",
      }}
      {...webDropHandlers}
    >
      {webDragOver && (
        <div
          data-testid="web-drop-overlay"
          className="absolute inset-0 z-20 flex items-center justify-center border-2 border-dashed pointer-events-none"
          style={{
            borderColor: "var(--ct-accent, #58a6ff)",
            background: "color-mix(in srgb, var(--ct-bg, #0d1117) 70%, transparent)",
          }}
        >
          <span className="text-sm font-mono">Drop files to attach</span>
        </div>
      )}
      <div
        ref={scrollRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          const dist = el.scrollHeight - (el.scrollTop + el.clientHeight);
          atBottomRef.current = dist < 50;
        }}
        // overscroll-contain: rubber-banding at the list edge must not chain into a page scroll — iOS dismisses the keyboard on page scrolls.
        className="flex-1 min-h-0 overflow-y-auto overscroll-contain p-3"
      >
        {touchMode && (
          <div
            aria-hidden="true"
            className="shrink-0"
            style={{ height: "calc(1.25rem + var(--ct-safe-top, 0px))" }}
          />
        )}
        {(hiddenMessageCount > 0 || earliestOffset > 0) && (
          <div className="sticky top-0 z-10 flex justify-center pb-2">
            <button
              type="button"
              onClick={() => void loadOlder()}
              disabled={loadingOlder}
              className="rounded px-2.5 py-1 text-xs font-mono hover:brightness-125 disabled:opacity-50"
              style={{
                background: "color-mix(in srgb, var(--ct-bg, #0d1117) 92%, black 8%)",
                color: "var(--ct-muted-fg)",
                border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 25%, transparent)",
              }}
            >
              {loadingOlder
                ? "Loading..."
                : hiddenMessageCount > 0
                  ? `Show older (${hiddenMessageCount})`
                  : "Load older"}
            </button>
          </div>
        )}
        {enrichedMessages.length === 0 && (
          <div className="opacity-50 text-center py-8 text-sm">
            No messages yet.
          </div>
        )}
        {enrichedMessages.map(({ msg, origin }) => {
          const isPending = msg.id.startsWith("opt-");
          let onOriginClick: (() => void) | undefined;
          let originProviderId: string | null | undefined;
          if (origin?.kind === "pane") {
            const idShort = origin.paneIdShort;
            const fullId = Object.keys(muxState.panes).find((id) => id.startsWith(idShort));
            if (fullId) {
              onOriginClick = () => setFocusedPane(fullId);
              originProviderId = muxState.panes[fullId]?.agent_type;
            }
          }
          return (
            <div
              key={msg.id}
              data-testid="chat-message"
              data-message-role={msg.type}
              style={isPending ? { opacity: 0.55 } : undefined}
              title={isPending ? "sending… (echo expected within 60s)" : undefined}
            >
              <MessageBubble
                message={msg}
                origin={origin}
                originAvatar={
                  origin?.kind === "pane" && originProviderId
                    ? <OriginProviderAvatar providerId={originProviderId} />
                    : undefined
                }
                onOriginClick={onOriginClick}
              />
            </div>
          );
        })}
      </div>
      <div
        className="shrink-0 p-2 border-t"
        style={{
          borderColor:
            "color-mix(in srgb, var(--ct-muted-fg, #8b949e) 30%, transparent)",
          // Driven by the real keyboard inset (visualViewport, see App.tsx): safe-area pad when closed, flat 0.5rem once the keyboard covers the home indicator. Focus alone (e.g. mic tap) must NOT collapse it — that drops the safe area with no keyboard present.
          paddingBottom: "var(--ct-composer-bottom-padding, calc(0.5rem + var(--ct-safe-bottom, 0px)))",
        }}
      >
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
          onRemove={(p) => setAttachments((prev) => prev.filter((x) => x !== p))}
          onToggleTerminalFallback={onToggleTerminalFallback}
          terminalActive={terminalHasAttention}
          pendingPrompt={pendingPrompt}
        />
        {/* Bug 4: relative wrapper so SlashPicker positions above textarea */}
        <div data-testid="composer-row" className="relative">
          <ComposerActions
            onAttach={addAttachments}
            onSend={() => void send()}
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
              // Never disabled/readOnly while sending: iOS hides the keyboard for both. Double-send is guarded by sendInFlightRef.
              className="block w-full min-h-5 flex-1 self-center resize-none bg-transparent py-0 pr-1 text-[15px] leading-5 outline-none placeholder:text-[var(--ct-muted-fg)]"
              style={{
                color: "var(--ct-fg, #c9d1d9)",
                border: "none",
                lineHeight: `${COMPOSER_MIN_HEIGHT}px`,
                margin: 0,
                overflowY: "hidden",
              }}
            />
          </ComposerActions>
        </div>
      </div>
    </div>
  );
}
