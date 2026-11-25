import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { DeferredLoader } from "@codeterm/chat-ui/DeferredLoader";
import { useLatestCallback } from "../../hooks/useLatestCallback";
import { ActivityDots } from "../shared/ActivityDots";
import { BusyState } from "../shared/BusyState";
import { Button } from "../shared/Button";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import {
  createHistorySentinelController,
  type ChatHistorySentinelConfig,
  type HistorySentinelController,
  type HistorySentinelObserverFactory,
} from "./chatHistorySentinelModel";

export type ChatHistoryLoadSource = "sentinel" | "manual";

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
  onLoadOlder: (source?: ChatHistoryLoadSource) => void;
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

  const { active, hasMore, loading, paused, focusSuspended, stalled } = config;
  useEffect(() => {
    controllerRef.current?.update({ active, hasMore, loading, paused, focusSuspended, stalled });
  }, [active, hasMore, loading, paused, focusSuspended, stalled]);

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
  const [buttonFocused, setButtonFocused] = useState(false);
  const buttonFocusedRef = useRef(false);
  const showHistoryRail = railsVisible && hasMore;
  const previousShowHistoryRailRef = useRef(showHistoryRail);
  const sentinelConfig: ChatHistorySentinelConfig = {
    active: active && railsVisible,
    hasMore,
    loading,
    paused,
    focusSuspended: buttonFocused,
    stalled,
  };
  const sentinelConfigRef = useRef(sentinelConfig);
  sentinelConfigRef.current = sentinelConfig;
  const handleAutoLoad = useLatestCallback(() => onLoadOlder("sentinel"), [onLoadOlder]);
  const controllerRef = useChatHistorySentinel({
    scrollRef,
    sentinelRef,
    mounted: showHistoryRail,
    sessionKey,
    config: sentinelConfig,
    onLoad: handleAutoLoad,
    createObserver,
  });

  useLayoutEffect(() => {
    const wasShown = previousShowHistoryRailRef.current;
    previousShowHistoryRailRef.current = showHistoryRail;
    if (!wasShown || showHistoryRail || !buttonFocusedRef.current) return;
    buttonFocusedRef.current = false;
    setButtonFocused(false);
    const scroller = scrollRef.current;
    if (!scroller) return;
    const ownerDoc = scroller.ownerDocument;
    if (ownerDoc.activeElement && ownerDoc.activeElement !== ownerDoc.body) return;
    const target = scroller.querySelector<HTMLElement>("[data-message-id]") ?? scroller;
    target.focus({ preventScroll: true });
  }, [scrollRef, showHistoryRail]);

  if (!showHistoryRail && !stalled) return null;

  const focusedLoading = loading && buttonFocused;

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
          <div className="sr-only focus-within:not-sr-only">
            <Button
              variant="ghost"
              size="sm"
              className="gap-2"
              data-testid="chat-load-older"
              aria-disabled={loading || undefined}
              aria-busy={focusedLoading || undefined}
              onFocus={() => {
                buttonFocusedRef.current = true;
                setButtonFocused(true);
                controllerRef.current?.update({ ...sentinelConfigRef.current, focusSuspended: true });
              }}
              onBlur={() => {
                buttonFocusedRef.current = false;
                setButtonFocused(false);
                controllerRef.current?.update({ ...sentinelConfigRef.current, focusSuspended: false });
              }}
              onClick={() => {
                if (loading) return;
                controllerRef.current?.notifyManualLoad();
                onLoadOlder("manual");
              }}
            >
              {focusedLoading && (
                <ActivityDots
                  size="small"
                  tone="accent"
                  rhythm="work"
                  data-testid="chat-history-loading-inline"
                />
              )}
              {olderMessageLabel}
            </Button>
          </div>
          {loading && !buttonFocused && (
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
