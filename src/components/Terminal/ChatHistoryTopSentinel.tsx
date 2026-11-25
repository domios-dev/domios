import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";
import { DeferredLoader } from "@codeterm/chat-ui/DeferredLoader";
import { useLatestCallback } from "../../hooks/useLatestCallback";
import { BusyState } from "../shared/BusyState";
import { Button } from "../shared/Button";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import {
  createHistorySentinelController,
  type ChatHistorySentinelConfig,
  type HistorySentinelController,
  type HistorySentinelObserverFactory,
} from "./chatHistorySentinelModel";

export interface ChatHistoryTopSentinelProps {
  scrollRef: RefObject<HTMLDivElement | null>;
  active: boolean;
  railsVisible: boolean;
  hasMore: boolean;
  loading: boolean;
  paused: boolean;
  stalled: boolean;
  olderMessageLabel: string;
  sessionKey: string;
  onLoadOlder: () => void;
  createObserver?: HistorySentinelObserverFactory;
}

function useChatHistorySentinel({
  scrollRef,
  sentinelRef,
  mounted,
  sessionKey,
  config,
  onLoad,
  createObserver,
}: {
  scrollRef: RefObject<HTMLDivElement | null>;
  sentinelRef: RefObject<HTMLDivElement | null>;
  mounted: boolean;
  sessionKey: string;
  config: ChatHistorySentinelConfig;
  onLoad: () => void;
  createObserver?: HistorySentinelObserverFactory;
}) {
  const controllerRef = useRef<HistorySentinelController | null>(null);
  const latestConfigRef = useRef(config);
  latestConfigRef.current = config;
  const latestOnLoad = useLatestCallback(onLoad, []);

  useEffect(() => {
    if (!mounted) return;
    const target = sentinelRef.current;
    if (!target) return;
    const controller = createHistorySentinelController({
      root: scrollRef.current,
      target,
      config: latestConfigRef.current,
      onLoad: latestOnLoad,
      createObserver,
    });
    controllerRef.current = controller;
    return () => {
      controller.dispose();
      if (controllerRef.current === controller) controllerRef.current = null;
    };
  }, [createObserver, latestOnLoad, mounted, scrollRef, sentinelRef, sessionKey]);

  const { active, hasMore, loading, paused, stalled } = config;
  useEffect(() => {
    controllerRef.current?.update({ active, hasMore, loading, paused, stalled });
  }, [active, hasMore, loading, paused, stalled]);

  return controllerRef;
}

export function ChatHistoryTopSentinel({
  scrollRef,
  active,
  railsVisible,
  hasMore,
  loading,
  paused,
  stalled,
  olderMessageLabel,
  sessionKey,
  onLoadOlder,
  createObserver,
}: ChatHistoryTopSentinelProps) {
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const buttonFocusedRef = useRef(false);
  const showHistoryRail = railsVisible && hasMore;
  const previousShowHistoryRailRef = useRef(showHistoryRail);
  const controllerRef = useChatHistorySentinel({
    scrollRef,
    sentinelRef,
    mounted: showHistoryRail,
    sessionKey,
    config: {
      active: active && railsVisible,
      hasMore,
      loading,
      paused,
      stalled,
    },
    onLoad: onLoadOlder,
    createObserver,
  });

  useLayoutEffect(() => {
    const wasShown = previousShowHistoryRailRef.current;
    previousShowHistoryRailRef.current = showHistoryRail;
    if (!wasShown || showHistoryRail || !buttonFocusedRef.current) return;
    buttonFocusedRef.current = false;
    const scroller = scrollRef.current;
    if (!scroller) return;
    const ownerDoc = scroller.ownerDocument;
    if (ownerDoc.activeElement && ownerDoc.activeElement !== ownerDoc.body) return;
    const target = scroller.querySelector<HTMLElement>("[data-message-id]") ?? scroller;
    target.focus({ preventScroll: true });
  }, [scrollRef, showHistoryRail]);

  if (!showHistoryRail && !stalled) return null;

  return (
    <div className="flex flex-col items-center">
      {showHistoryRail && (
        <div className="relative flex h-8 w-full items-center justify-center">
          <div
            ref={sentinelRef}
            aria-hidden="true"
            data-testid="chat-history-sentinel"
            className="pointer-events-none absolute inset-0"
          />
          <div className={loading ? "sr-only" : "sr-only focus-within:not-sr-only"}>
            <Button
              variant="ghost"
              size="sm"
              data-testid="chat-load-older"
              aria-disabled={loading || undefined}
              onFocus={() => {
                buttonFocusedRef.current = true;
              }}
              onBlur={() => {
                buttonFocusedRef.current = false;
              }}
              onClick={() => {
                if (loading) return;
                controllerRef.current?.notifyManualLoad();
                onLoadOlder();
              }}
            >
              {olderMessageLabel}
            </Button>
          </div>
          {loading && (
            <DeferredLoader
              state="present"
              className="pointer-events-none absolute inset-0 flex items-center justify-center"
            >
              <BusyState
                inline
                hideLabel
                label="Loading earlier messages"
                phase="work"
                dotSize="small"
                data-testid="chat-history-loading"
              />
            </DeferredLoader>
          )}
        </div>
      )}
      {stalled && (
        <div className="w-full py-1">
          <InlineErrorNotice error="Older history could not be read." operation="loading older messages" />
        </div>
      )}
    </div>
  );
}
