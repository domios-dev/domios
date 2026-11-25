export const HISTORY_SENTINEL_ROOT_MARGIN = "120px 0px 0px 0px";

export interface ChatHistorySentinelState {
  intersecting: boolean;
  armed: boolean;
}

export interface ChatHistorySentinelConfig {
  active: boolean;
  hasMore: boolean;
  loading: boolean;
  paused: boolean;
  stalled: boolean;
}

export type ChatHistorySentinelEvent =
  | { kind: "intersection"; isIntersecting: boolean }
  | { kind: "sync" }
  | { kind: "manual_load" };

export interface ChatHistorySentinelStep {
  state: ChatHistorySentinelState;
  shouldLoad: boolean;
}

export interface HistorySentinelObserverOptions {
  root: Element | null;
  rootMargin: string;
  threshold: number;
}

export interface HistorySentinelObserver {
  observe(target: Element): void;
  disconnect(): void;
}

export type HistorySentinelObserverFactory = (
  callback: (isIntersecting: boolean) => void,
  options: HistorySentinelObserverOptions,
) => HistorySentinelObserver;

export function createChatHistorySentinelState(): ChatHistorySentinelState {
  return { intersecting: false, armed: true };
}

export function canTriggerHistorySentinel(config: ChatHistorySentinelConfig): boolean {
  return config.active && config.hasMore && !config.loading && !config.paused && !config.stalled;
}

export function reduceChatHistorySentinel(
  state: ChatHistorySentinelState,
  event: ChatHistorySentinelEvent,
  config: ChatHistorySentinelConfig,
): ChatHistorySentinelStep {
  if (!config.active || !config.hasMore) {
    return { state: createChatHistorySentinelState(), shouldLoad: false };
  }
  if (event.kind === "manual_load") {
    return {
      state: { intersecting: state.intersecting, armed: state.intersecting ? false : state.armed },
      shouldLoad: false,
    };
  }
  if (event.kind === "intersection") {
    if (!event.isIntersecting) {
      return { state: { intersecting: false, armed: true }, shouldLoad: false };
    }
    if (config.loading) {
      return { state: { intersecting: true, armed: false }, shouldLoad: false };
    }
    if (state.armed && canTriggerHistorySentinel(config)) {
      return { state: { intersecting: true, armed: false }, shouldLoad: true };
    }
    return { state: { intersecting: true, armed: state.armed }, shouldLoad: false };
  }
  if (config.loading && state.intersecting) {
    return { state: { intersecting: true, armed: false }, shouldLoad: false };
  }
  if (state.intersecting && state.armed && canTriggerHistorySentinel(config)) {
    return { state: { intersecting: true, armed: false }, shouldLoad: true };
  }
  return { state, shouldLoad: false };
}

export function browserHistorySentinelObserverFactory(
  callback: (isIntersecting: boolean) => void,
  options: HistorySentinelObserverOptions,
): HistorySentinelObserver {
  if (typeof IntersectionObserver === "undefined") {
    return { observe() {}, disconnect() {} };
  }
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) callback(entry.isIntersecting);
  }, options);
  return {
    observe: (target) => observer.observe(target),
    disconnect: () => observer.disconnect(),
  };
}

export interface HistorySentinelController {
  update(config: ChatHistorySentinelConfig): void;
  notifyManualLoad(): void;
  dispose(): void;
}

export function createHistorySentinelController({
  root,
  target,
  config,
  onLoad,
  createObserver = browserHistorySentinelObserverFactory,
  rootMargin = HISTORY_SENTINEL_ROOT_MARGIN,
}: {
  root: Element | null;
  target: Element;
  config: ChatHistorySentinelConfig;
  onLoad: () => void;
  createObserver?: HistorySentinelObserverFactory;
  rootMargin?: string;
}): HistorySentinelController {
  let state = createChatHistorySentinelState();
  let current = config;
  let observer: HistorySentinelObserver | null = null;
  let disposed = false;

  const detach = () => {
    if (observer) {
      observer.disconnect();
      observer = null;
    }
    state = createChatHistorySentinelState();
  };

  const attach = () => {
    if (observer || disposed || !current.active || !current.hasMore) return;
    const instance = createObserver((isIntersecting) => {
      if (disposed || observer !== instance) return;
      const step = reduceChatHistorySentinel(state, { kind: "intersection", isIntersecting }, current);
      state = step.state;
      if (step.shouldLoad) onLoad();
    }, { root, rootMargin, threshold: 0 });
    observer = instance;
    instance.observe(target);
  };

  attach();

  return {
    update(nextConfig) {
      if (disposed) return;
      current = nextConfig;
      if (!current.active || !current.hasMore) {
        detach();
        return;
      }
      if (!observer) {
        attach();
        return;
      }
      const step = reduceChatHistorySentinel(state, { kind: "sync" }, current);
      state = step.state;
      if (step.shouldLoad) onLoad();
    },
    notifyManualLoad() {
      if (disposed) return;
      state = reduceChatHistorySentinel(state, { kind: "manual_load" }, current).state;
    },
    dispose() {
      disposed = true;
      detach();
    },
  };
}
