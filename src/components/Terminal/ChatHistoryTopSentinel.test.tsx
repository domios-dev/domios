import { act, createRef, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import { ChatHistoryTopSentinel, type ChatHistoryTopSentinelProps } from "./ChatHistoryTopSentinel";
import type { HistorySentinelObserverFactory } from "./chatHistorySentinelModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

interface RecordedObserver {
  target: Element | null;
  disconnected: boolean;
  deliver(isIntersecting: boolean): void;
}

function createObserverPort() {
  const instances: RecordedObserver[] = [];
  const createObserver: HistorySentinelObserverFactory = (callback) => {
    const entry: RecordedObserver = {
      target: null,
      disconnected: false,
      deliver(isIntersecting) {
        callback(isIntersecting);
      },
    };
    instances.push(entry);
    return {
      observe(target) {
        entry.target = target;
      },
      disconnect() {
        entry.disconnected = true;
      },
    };
  };
  return { createObserver, instances, latest: () => instances[instances.length - 1] };
}

let root: Root | undefined;
afterEach(async () => {
  await act(async () => root?.unmount());
  root = undefined;
  document.body.replaceChildren();
});

async function mount(overrides: Partial<ChatHistoryTopSentinelProps> = {}, rows?: ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  const scrollRef = createRef<HTMLDivElement>();
  const port = createObserverPort();
  let loads = 0;

  const baseProps: ChatHistoryTopSentinelProps = {
    scrollRef,
    active: true,
    railsVisible: true,
    hasMore: true,
    loading: false,
    paused: false,
    stalled: false,
    olderMessageLabel: "Load previous 100 messages",
    sessionKey: "tab-1:session-1",
    onLoadOlder: () => {
      loads++;
    },
    createObserver: port.createObserver,
    ...overrides,
  };

  root = createRoot(host);
  const render = async (next: Partial<ChatHistoryTopSentinelProps> = {}, nextRows = rows) => {
    const props = { ...baseProps, ...next };
    await act(async () => {
      root!.render(
        <div ref={scrollRef} tabIndex={-1}>
          <ChatHistoryTopSentinel {...props} />
          {nextRows}
        </div>,
      );
    });
  };

  await render();
  return { host, scrollRef, port, render, loads: () => loads };
}

describe("ChatHistoryTopSentinel", () => {
  it("renders nothing when history is exhausted", async () => {
    const { host, port } = await mount({ hasMore: false });
    expect(host.textContent).toBe("");
    expect(host.querySelector('[data-testid="chat-load-older"]')).toBeNull();
    expect(host.querySelector('[data-testid="chat-history-sentinel"]')).toBeNull();
    expect(host.querySelector('[role="status"]')).toBeNull();
    expect(port.instances).toHaveLength(0);
  });

  it("loads once per intersection reach, stops at exhaustion, and stops when hidden", async () => {
    const { port, render, loads } = await mount();
    expect(port.instances).toHaveLength(1);

    await act(async () => port.latest().deliver(true));
    expect(loads()).toBe(1);

    await act(async () => port.latest().deliver(true));
    expect(loads()).toBe(1);

    await act(async () => port.latest().deliver(false));
    await render({ active: false });
    expect(port.latest().disconnected).toBe(true);
    await act(async () => port.latest().deliver(true));
    expect(loads()).toBe(1);

    await render({ active: true });
    expect(port.instances).toHaveLength(2);
    await act(async () => port.latest().deliver(true));
    expect(loads()).toBe(2);

    await render({ hasMore: false });
    expect(port.latest().disconnected).toBe(true);
    await act(async () => port.latest().deliver(true));
    expect(loads()).toBe(2);
  });

  it("exposes a keyboard-reachable control with visible label-in-name and a single polite deferred loader", async () => {
    const { host, render, loads } = await mount();
    const button = host.querySelector<HTMLButtonElement>('[data-testid="chat-load-older"]');
    expect(button).not.toBeNull();
    expect(button?.textContent).toBe("Load previous 100 messages");
    expect(button?.hasAttribute("aria-label")).toBe(false);

    await act(async () => button!.click());
    expect(loads()).toBe(1);

    await render({ loading: true });
    const statuses = host.querySelectorAll('[role="status"]');
    expect(statuses).toHaveLength(1);
    const status = host.querySelector('[data-testid="chat-history-loading"]');
    expect(status?.getAttribute("role")).toBe("status");
    expect(status?.getAttribute("aria-live")).toBe("polite");
    expect(status?.getAttribute("aria-label")).toBe("Loading earlier messages");
    expect(status?.closest('[data-transition-state="present"]')).not.toBeNull();

    const loadingButton = host.querySelector<HTMLButtonElement>('[data-testid="chat-load-older"]');
    expect(loadingButton?.getAttribute("aria-disabled")).toBe("true");
    await act(async () => loadingButton!.click());
    expect(loads()).toBe(1);
  });

  it("moves focus to the first transcript row when a focused load control unmounts at exhaustion", async () => {
    const { host, render } = await mount(
      {},
      <div data-message-id="m-1" tabIndex={-1}>
        First row
      </div>,
    );
    const button = host.querySelector<HTMLButtonElement>('[data-testid="chat-load-older"]')!;
    await act(async () => button.focus());
    expect(document.activeElement).toBe(button);

    await render({ loading: true });
    expect(document.activeElement).toBe(button);

    await render({ loading: false, hasMore: false });
    const firstRow = host.querySelector<HTMLElement>('[data-message-id="m-1"]');
    expect(document.activeElement).toBe(firstRow);
  });

  it("falls back to focusing the transcript container when no message row exists at exhaustion", async () => {
    const { host, scrollRef, render } = await mount();
    const button = host.querySelector<HTMLButtonElement>('[data-testid="chat-load-older"]')!;
    await act(async () => button.focus());
    expect(document.activeElement).toBe(button);

    await render({ hasMore: false });
    expect(document.activeElement).toBe(scrollRef.current);
  });

  it("keeps one observer across re-renders with new callback identities and does not re-trigger", async () => {
    const { port, render, loads } = await mount();
    expect(port.instances).toHaveLength(1);

    await act(async () => port.latest().deliver(true));
    expect(loads()).toBe(1);

    let secondLoads = 0;
    await render({
      onLoadOlder: () => {
        secondLoads++;
      },
    });
    expect(port.instances).toHaveLength(1);
    await act(async () => port.latest().deliver(true));
    expect(loads()).toBe(1);
    expect(secondLoads).toBe(0);

    await act(async () => port.latest().deliver(false));
    await act(async () => port.latest().deliver(true));
    expect(secondLoads).toBe(1);
  });

  it("keeps the stalled_cursor error notice and halts auto-loading while stalled", async () => {
    const { host, port, loads } = await mount({ stalled: true });
    expect(host.textContent).toContain("Older history could not be read.");
    await act(async () => port.latest().deliver(true));
    expect(loads()).toBe(0);
  });
});
