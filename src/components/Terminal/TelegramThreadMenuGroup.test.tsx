import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { __setInvokeForTest } from "../../lib/bridge";
import { TabActionsMenu } from "./TabActionsMenu";
import {
  closeTelegramThread,
  fetchTelegramThreadState,
  openTelegramThread,
  TELEGRAM_THREAD_LABELS,
  type TelegramThreadMenuState,
} from "./telegramThreadActions";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  const mounted = root;
  if (mounted) await act(async () => mounted.unmount());
  host?.remove();
  host = null;
  root = null;
  __setInvokeForTest(null);
});

const RUNNING_BOUND: TelegramThreadMenuState = {
  bridgeActive: true,
  topics: "available",
  bound: true,
  link: "https://t.me/example_bot/42",
};

async function openMenu(telegramThread: TelegramThreadMenuState | null) {
  const onOpen = vi.fn();
  const onClose = vi.fn();
  const onMenuOpen = vi.fn();
  host = document.createElement("div");
  document.body.appendChild(host);
  const mounted = createRoot(host);
  root = mounted;
  await act(async () => {
    mounted.render(
      <TabActionsMenu
        sessionId={null}
        canReconnect={false}
        canRestart={false}
        reconnecting={false}
        restarting={false}
        onReconnect={() => {}}
        onRestart={() => {}}
        onError={() => {}}
        telegramThread={telegramThread}
        onMenuOpen={onMenuOpen}
        onOpenTelegramThread={onOpen}
        onCloseTelegramThread={onClose}
      />,
    );
  });
  const trigger = host.querySelector<HTMLButtonElement>('[data-testid="tab-actions-trigger"]');
  expect(trigger).not.toBeNull();
  await act(async () => trigger?.click());
  return { onOpen, onClose, onMenuOpen };
}

function item(testId: string): HTMLButtonElement | null {
  return document.querySelector<HTMLButtonElement>(`[data-testid="${testId}"]`);
}

function hintOf(button: HTMLButtonElement | null): string | null {
  const describedBy = button?.getAttribute("aria-describedby");
  return describedBy ? (document.getElementById(describedBy)?.textContent ?? null) : null;
}

describe("Telegram thread items in the tab actions menu", () => {
  it("offers open and, when bound, close inside the menu, each calling its own handler", async () => {
    const { onOpen, onMenuOpen } = await openMenu(RUNNING_BOUND);
    expect(onMenuOpen).toHaveBeenCalledTimes(1);
    const group = document.querySelector('[data-testid="tab-telegram-thread-group"]');
    expect(group?.closest('[role="menu"]')).not.toBeNull();
    const open = item("tab-telegram-thread-open");
    expect(open?.getAttribute("role")).toBe("menuitem");
    expect(open?.getAttribute("aria-disabled")).toBeNull();
    expect(hintOf(open)).toBeNull();
    await act(async () => open?.click());
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("routes close through its handler", async () => {
    const { onClose, onOpen } = await openMenu(RUNNING_BOUND);
    const close = item("tab-telegram-thread-close");
    expect(close?.getAttribute("role")).toBe("menuitem");
    await act(async () => close?.click());
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("keeps open focusable but inert with the bridge-off hint while the bridge is off", async () => {
    const { onOpen } = await openMenu({ bridgeActive: false, topics: "unknown", bound: false, link: null });
    const open = item("tab-telegram-thread-open");
    expect(open?.getAttribute("aria-disabled")).toBe("true");
    expect(hintOf(open)).toBe(TELEGRAM_THREAD_LABELS.bridgeOff);
    await act(async () => open?.click());
    expect(onOpen).not.toHaveBeenCalled();
    expect(item("tab-telegram-thread-close")).toBeNull();
  });

  it("shows the BotFather guidance when topics are unavailable and still lets the user retry", async () => {
    const { onOpen } = await openMenu({ bridgeActive: true, topics: "unavailable", bound: false, link: null });
    const open = item("tab-telegram-thread-open");
    expect(hintOf(open)).toBe(TELEGRAM_THREAD_LABELS.topicsDisabled);
    await act(async () => open?.click());
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("renders no group before the state is known", async () => {
    await openMenu(null);
    expect(document.querySelector('[data-testid="tab-actions-menu"]')).not.toBeNull();
    expect(document.querySelector('[data-testid="tab-telegram-thread-group"]')).toBeNull();
  });

  it("reads state and performs open and close through the bridge thread commands only", async () => {
    const calls: Array<[string, unknown]> = [];
    __setInvokeForTest((async (command: string, args?: unknown) => {
      calls.push([command, args]);
      if (command === "bridge_thread_state") {
        return { bridge_active: true, topics: "unavailable", bound: false, link: null };
      }
      if (command === "bridge_thread_open") return { link: "https://t.me/example_bot/7" };
      return { outcomes: [] };
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
    }) as any);

    expect(await fetchTelegramThreadState("tab-7")).toEqual({
      bridgeActive: true,
      topics: "unavailable",
      bound: false,
      link: null,
    });
    expect(await openTelegramThread("tab-7")).toBe("https://t.me/example_bot/7");
    await closeTelegramThread("tab-7");

    expect(calls).toEqual([
      ["bridge_thread_state", { tabId: "tab-7" }],
      ["bridge_thread_open", { tabId: "tab-7" }],
      ["bridge_thread_close", { tabId: "tab-7" }],
    ]);
  });
});
