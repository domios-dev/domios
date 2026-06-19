import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { MessageBubble } from "@codeterm/chat-ui";
import type { InteractiveAnswerHandler } from "@codeterm/chat-ui";
import { AgentIcon } from "../AgentIcon";
import { useMultiplexer } from "../../store/multiplexer";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";
import type { PendingPrompt } from "../../lib/types";
import { ChatComposer } from "./ChatComposer";
import { useAttachments } from "./useAttachments";
import { useChatSession } from "./useChatSession";
import { ChatModelBubble } from "../Extensions/ChatModelBubble";

interface Props {
  paneId: string;
  onError?: (msg: string) => void;
  onOpenTerminalFallback?: () => void;
  onToggleTerminalFallback?: () => void;
  uiFontScale?: number;
  /** Pane is backed by a plugin chatBackend (no PTY). Sends route through the
   *  pane send seam, and PTY-only affordances (terminal fallback, screen poll)
   *  are suppressed. */
  chatBackend?: boolean;
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

export function PaneSessionChat({
  paneId,
  onError,
  onOpenTerminalFallback,
  onToggleTerminalFallback,
  uiFontScale,
  chatBackend = false,
}: Props) {
  const touchMode = useTouchOptimizedUi();
  const scrollRef = useRef<HTMLDivElement>(null);
  const atBottomRef = useRef(true);
  const terminalTextRef = useRef<string | null>(null);
  const terminalActiveTimerRef = useRef<number | null>(null);
  const chatRootRef = useRef<HTMLDivElement>(null);
  const [terminalActive, setTerminalActive] = useState(false);

  const { state: muxState, setFocusedPane, addNotification } = useMultiplexer();
  const pane = muxState.panes[paneId];
  const paneProvider = pane?.agent_type ?? null;
  const agentSessionId = pane?.agent_session_id ?? null;
  const currentPtyId = pane?.pty_id ?? null;
  const chatBacking = pane?.backing?.kind === "chat_backend" ? pane.backing : null;
  const pendingPrompt: PendingPrompt | null = pane?.pending_prompt ?? null;
  const terminalHasAttention = terminalActive || Boolean(pendingPrompt);

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

  const {
    loading,
    error,
    allMessages,
    enrichedMessages,
    hiddenMessageCount,
    loadOlder,
    addOptimistic,
    updateOptimistic,
  } = useChatSession({ paneId, agentSessionId, onError });

  const [answeredInteractiveIds, setAnsweredInteractiveIds] = useState<Set<string>>(new Set());

  // Route an interactive-widget answer back to the agent through the SAME send
  // path the composer uses: write_pty_string with raw=false → backend bracket
  // paste + delayed CR, which handles the TUI-menu / long-message no-auto-submit
  // case (#155). We echo the answer optimistically so it appears as a user turn.
  const handleAnswerInteractive = useCallback<InteractiveAnswerHandler>(
    async (text, element) => {
      const answer = text.trim();
      if (!answer) return;
      if (!currentPtyId) {
        showComposerError("Pane has no PTY");
        throw new Error("Pane has no PTY");
      }
      setAnsweredInteractiveIds((prev) => new Set(prev).add(element.id));
      addOptimistic(answer);
      try {
        await invoke("write_pty_string", { ptyId: currentPtyId, data: answer + "\r", raw: false });
      } catch (e) {
        setAnsweredInteractiveIds((prev) => {
          const next = new Set(prev);
          next.delete(element.id);
          return next;
        });
        showComposerError(e instanceof Error ? e.message : String(e));
        throw e;
      }
    },
    [currentPtyId, addOptimistic, showComposerError],
  );

  const {
    attachments,
    addAttachments,
    removeAttachment,
    clearAttachments,
    handlePasteEvent,
    webDragOver,
    webDropHandlers,
  } = useAttachments({
    chatRootRef,
    enabled: !loading && !error,
    onError: showComposerError,
  });

  const resolvePaneName = useMemo(() => {
    const byShort = new Map<string, string>();
    for (const ws of muxState.workspaces) {
      for (const tab of ws.tabs) {
        const name = tab.title?.replace(/^[✻✳✴✵✶✷✸✹✺✻✼✽✾✿❀❁❂❃❇❈❉❊❋*]\s*/u, "").trim();
        if (name && tab.pane_id) byShort.set(tab.pane_id.slice(0, 8), name);
      }
    }
    return (paneRef: string): string | undefined => byShort.get(paneRef.slice(0, 8));
  }, [muxState.workspaces]);

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

  useEffect(() => {
    if (!scrollRef.current) return;
    if (atBottomRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [allMessages]);

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
      data-testid="pane-session-chat"
      data-pane-id={paneId}
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
        className="flex-1 min-h-0 overflow-y-auto overscroll-contain p-3"
        data-testid="pane-session-chat-messages"
      >
        {touchMode && (
          <div
            aria-hidden="true"
            className="shrink-0"
            style={{ height: "calc(1.25rem + var(--ct-safe-top, 0px))" }}
          />
        )}
        {hiddenMessageCount > 0 && (
          <div className="sticky top-0 z-10 flex justify-center pb-2">
            <button
              type="button"
              data-testid="chat-load-older"
              onClick={() => void loadOlder()}
              className="rounded px-2.5 py-1 text-xs font-mono hover:brightness-125 disabled:opacity-50"
              style={{
                background: "color-mix(in srgb, var(--ct-bg, #0d1117) 92%, black 8%)",
                color: "var(--ct-muted-fg)",
                border: "1px solid color-mix(in srgb, var(--ct-muted-fg, #8b949e) 25%, transparent)",
              }}
            >
              {`Show older (${hiddenMessageCount})`}
            </button>
          </div>
        )}
        {enrichedMessages.length === 0 && (
          <div className="opacity-50 text-center py-8 text-sm" data-testid="chat-empty-state">
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
              title={isPending ? "sending… (clears when the agent echoes it back)" : undefined}
            >
              <MessageBubble
                message={msg}
                origin={origin}
                resolvePaneName={resolvePaneName}
                fontScale={uiFontScale}
                onAnswerInteractive={handleAnswerInteractive}
                answeredInteractiveIds={answeredInteractiveIds}
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
      <ChatComposer
        paneId={paneId}
        paneProvider={paneProvider}
        currentPtyId={currentPtyId}
        chatBackend={chatBackend}
        pendingPrompt={pendingPrompt}
        terminalHasAttention={terminalHasAttention}
        attachments={attachments}
        addAttachments={addAttachments}
        removeAttachment={removeAttachment}
        clearAttachments={clearAttachments}
        handlePasteEvent={handlePasteEvent}
        addOptimistic={addOptimistic}
        updateOptimistic={updateOptimistic}
        showComposerError={showComposerError}
        onOpenTerminalFallback={chatBackend ? undefined : onOpenTerminalFallback}
        onToggleTerminalFallback={chatBackend ? undefined : onToggleTerminalFallback}
        modelBubble={chatBackend && chatBacking ? (
          <ChatModelBubble pluginId={chatBacking.plugin_id} sessionId={chatBacking.session_id} />
        ) : undefined}
      />
    </div>
  );
}
