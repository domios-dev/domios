import "../../../../src/components/Usage/styles.css";
import { lazy, Suspense, useState, useCallback, useEffect, useLayoutEffect, useMemo, useRef, type MutableRefObject, type Ref } from "react";
import { reduceToolEvents } from "@codeterm/chat-core";
import { MessageBubble } from "@codeterm/chat-ui";
import { chatMessageAccessibleName } from "../../../../src/components/Terminal/chatMessageAccessibleName";
import { chatLayoutTokens } from "../../../../src/components/Terminal/chatLayoutTokens";
import { ChatMessageRow, type MessageRowHandlers } from "../../../../src/components/Terminal/ChatMessageRow";
import { TabTopBar, type HeaderTabMenu } from "../../../../src/components/Terminal/TabTopBar";
import type { TabUiMode } from "../../../../src/lib/chatUiMode";
import { ComposerPreviewContext } from "../../../../src/components/Terminal/ComposerActions";
import { VoiceRecordingBar } from "../../../../src/components/Terminal/voice/VoiceRecordingBar";
import { Mic, Square } from "lucide-react";
import { AgentIcon } from "../../../../src/components/AgentIcon";
import { Button } from "../../../../src/components/shared/Button";
import { publishCurrentTabUiMode } from "../../../../src/lib/clientTabUiMode";
import { getAgentDescriptor, listAgentDescriptors } from "../../../../src/lib/agentDescriptorCache";
import type { Tab, TabStatus } from "../../../../src/lib/types";
import { ChatBottomBar, typeInto } from "../../gallery/ChatBottomBar";
import { ProductProviders } from "../../gallery/fixtures";
import type { BeatFrame } from "./beat";
import { HELM_ORIGINS } from "./story";

const noop = () => undefined;
const SampleTerminal = lazy(() => import("./SampleTerminal"));

const HANDLERS: MessageRowHandlers = {
  clipFor: () => null,
  partsFor: () => [],
  selectedFor: () => false,
  cursorFor: () => false,
  joinNextFor: () => false,
  select: noop,
  rowMouseDown: noop,
  toggle: noop,
  range: noop,
  endRange: noop,
  quote: noop,
  ask: noop,
};

function headerMenu(tab: Tab, status: TabStatus, notInDemo: (what: string) => void): HeaderTabMenu {
  return {
    tab,
    context: { status, descriptor: getAgentDescriptor(tab.agent_type), descriptors: listAgentDescriptors(), activity: null, orchestrators: [] },
    onRename: () => notInDemo("Renaming a tab"),
    onClose: () => notInDemo("Closing a tab"),
  };
}

export interface ChatPaneProps {
  tab: Tab;
  status: TabStatus;
  frame: BeatFrame;
  workspaceId: string;
  composerRef: Ref<HTMLDivElement>;
  onFocusTab: (tabRef: string) => void;
  onOpenTask: (taskId: string | null) => void;
  onOpenJobs: () => void;
  notInDemo: (what: string) => void;
  voice: "recording" | "transcribing" | null;
  voiceSeconds: number;
  onVoice: () => void;
  onCancelVoice: () => void;
}

/** TabContent's column for a chat tab: TabTopBar over TabSessionChat's message surface and its bottom bar, the composer inert. */
export function ChatPane({ tab, status, frame, workspaceId, composerRef, onFocusTab, onOpenTask, onOpenJobs, notInDemo, voice, voiceSeconds, onVoice, onCancelVoice }: ChatPaneProps) {
  const cancelVoice = useRef<HTMLDivElement>(null);
  const [views, setViews] = useState<Record<string, TabUiMode>>({});
  const terminal = views[tab.id] === "terminal";
  const items = useMemo(() => reduceToolEvents(frame.messages), [frame.messages]);
  const log = useRef<HTMLDivElement>(null);
  // The person's line goes into the real composer through its own input event, one frame's prefix at a time.
  const composerBox = useRef<HTMLDivElement | null>(null);
  const setComposer = useCallback((el: HTMLDivElement | null) => {
    composerBox.current = el;
    if (typeof composerRef === "function") composerRef(el);
    else if (composerRef) (composerRef as MutableRefObject<HTMLDivElement | null>).current = el;
  }, [composerRef]);
  useLayoutEffect(() => {
    const field = composerBox.current?.querySelector<HTMLTextAreaElement>('textarea[data-testid="chat-composer"]');
    if (field && field.value !== frame.draft) typeInto(field, frame.draft);
  });
  // The app pins the log to its newest row while it grows, unless the reader scrolled up.
  const pinned = useRef(true);
  useLayoutEffect(() => {
    pinned.current = true;
  }, [tab.id]);
  useLayoutEffect(() => {
    const el = log.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [frame.key, tab.id]);
  const onScroll = () => {
    const el = log.current;
    if (el) pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  };
  // The real mode switch selects a local terminal transcript or the chat view.
  useEffect(() => {
    const onMode = (event: Event) => {
      const detail = (event as CustomEvent<{ tabId?: string; mode?: string }>).detail;
      if (detail?.tabId !== tab.id || (detail.mode !== "terminal" && detail.mode !== "chat")) return;
      const mode = detail.mode;
      setViews((previous) => ({ ...previous, [tab.id]: mode }));
      publishCurrentTabUiMode(tab.id, mode);
    };
    window.addEventListener("codeterm-tab-ui-mode", onMode);
    return () => window.removeEventListener("codeterm-tab-ui-mode", onMode);
  }, [tab.id]);
  useEffect(() => { publishCurrentTabUiMode(tab.id, terminal ? "terminal" : "chat"); }, [tab.id, terminal]);
  const openPlan = (_planId: string, focusId?: string, kind?: string) => onOpenTask(kind === "task" && focusId ? focusId : null);

  return <div data-tab-id={tab.id} className="w-full h-full relative group/Tab">
    <div className="flex h-full w-full flex-col">
      <ProductProviders tabs={[tab]} workspaceId={workspaceId}>
        <TabTopBar
          tabId={tab.id}
          shellTab={tab}
          cwd={tab.cwd ?? null}
          workspaceId={workspaceId}
          showSwitch
          usageSource={tab.agent_type ? { kind: "meter", provider: tab.agent_type, trackingProvider: tab.agent_type, borrowedAccount: false } : null}
          usageVisible
          usageModelId={tab.agent_session_model ?? null}
          tabMenu={headerMenu(tab, status, notInDemo)}
          isActive
        />
      </ProductProviders>
      <div className="relative flex min-h-0 flex-1 flex-col" style={{ background: "var(--ct-bg)" }}>
        {terminal ? <Suspense fallback={<p className="p-4 text-xs">Opening terminal…</p>}>
          <SampleTerminal key={tab.id} cwd={tab.cwd ?? null} messages={frame.messages} />
        </Suspense> : <div className="min-h-0 flex-1">
          <div className="ct-chat-shell w-full h-full flex flex-col relative" data-testid="tab-session-chat" style={{ ...chatLayoutTokens().variables, background: "var(--ct-bg, #0d1117)", color: "var(--ct-fg, #c9d1d9)" }}>
            <div className="relative flex-1 min-h-0">
              {items.length === 0 && (
                <div className="chat-empty-state pointer-events-none absolute inset-0 flex items-center justify-center select-none"
                  data-testid="chat-empty-state" aria-hidden>
                  <span className="chat-empty-mark">
                    <AgentIcon providerId={tab.agent_type} className="chat-empty-glyph text-[56px]" />
                  </span>
                </div>
              )}
              <div ref={log} onScroll={onScroll} className="ct-chat-surface absolute inset-0 overflow-y-auto overflow-x-hidden px-5 py-3 select-text" data-testid="tab-session-chat-messages" role="log" aria-label="Conversation">
                <div className="flex min-h-full flex-col justify-end">
                  {items.map((item) => {
                    const message = item.kind === "message" ? item.message : item.event.message;
                    return (
                      <ChatMessageRow key={message.id} msg={message} ariaLabel={chatMessageAccessibleName(message.type, message.content)} selected={false} joinNext={false} selecting={false} cursor={false} handlers={HANDLERS}>
                        <MessageBubble
                          message={message}
                          item={item.kind === "tool" ? item : undefined}
                          origin={HELM_ORIGINS[message.id]}
                          streaming={frame.streaming === message.id}
                          onFocusTab={onFocusTab}
                          onOpenPlan={openPlan}
                          onOriginClick={noop}
                          onOpenJobs={onOpenJobs}
                          onStopReminder={() => notInDemo("Stopping a reminder")}
                        />
                      </ChatMessageRow>
                    );
                  })}
                </div>
              </div>
            </div>
            <div ref={setComposer} data-film-anchor="composer">
              <ComposerPreviewContext value={{
                action: <Button size="sm" data-testid="demo-voice-button" className="demo-voice-button"
                  aria-label={voice === "recording" ? "Finish voice note" : "Record a sample voice note"}
                  disabled={voice === "transcribing"} onClick={onVoice}>{voice === "recording" ? <Square size={15} /> : <Mic size={16} />}</Button>,
                inputOverlay: voice === "recording"
                  ? <VoiceRecordingBar mode="locked" peaks={Array.from({ length: 34 }, (_, index) =>
                    0.15 + Math.abs(Math.sin(index * 2.7 + voiceSeconds * 10)) * 0.72)}
                    cancelHintRef={cancelVoice} onCancel={onCancelVoice} />
                  : voice === "transcribing"
                    ? <span role="status" className="pl-2 text-sm text-[var(--ct-muted-fg)]">Transcribing voice note…</span>
                    : undefined,
              }}>
                <ChatBottomBar key={tab.id} tab={tab} status={status} workspaceId={workspaceId} showStatus />
              </ComposerPreviewContext>
            </div>
          </div>
        </div>}
      </div>
    </div>
  </div>;
}
