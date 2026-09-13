import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { NeedsYouPanel } from "../Canvas/missionControl/BoardCards";
import type { HeldForDecision } from "../Canvas/missionControl/missionControlState";
import { useChatMessageReveal } from "../Terminal/useChatMessageReveal";
import { useChatScroll } from "../Terminal/useChatScroll";
import { canvasFocusStore } from "../Canvas/canvasFocusStore";
import { clearChatReveal, currentChatReveal, requestChatMessageReveal } from "../../lib/chatMessageReveal";
import { getClientTabUiMode, resolveInitialTabUiMode, setClientTabUiMode } from "../../lib/clientTabUiMode";
import { jumpToChatMessage } from "./jumpToChatMessage";
import {
  DEEP_LINK_UNAVAILABLE_COPY,
  needsYouNotificationItems,
  resolveDeepLink,
} from "../../lib/notificationDeepLink";
import type { Notification, Tab, Workspace } from "../../lib/types";
import type { NeedsYouDeepLinks } from "./NeedsYouNotificationRow";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const TEAM = "team-a";
const TARGET_TAB = "tab-worker";
const TARGET_MESSAGE = "needs-you-message-1";
const SHARED_TEXT = "Approve the rollout?";

function tab(id: string, teamId: string | null, agentType: string | null = "claude"): Tab {
  return {
    id,
    workspace_id: "ws-1",
    team_id: teamId,
    agent_type: agentType,
    title: id,
    tab_type: "Terminal",
    pty_id: null,
    cwd: null,
  } as Tab;
}

const SHELL_TAB = "tab-shell";

const TABS: Record<string, Tab> = {
  "tab-orchestrator": tab("tab-orchestrator", TEAM),
  [TARGET_TAB]: tab(TARGET_TAB, TEAM),
  "tab-foreign": tab("tab-foreign", "team-b"),
  [SHELL_TAB]: tab(SHELL_TAB, TEAM, null),
};

const WORKSPACES = [
  { id: "ws-1", tabs: Object.keys(TABS) },
] as unknown as Workspace[];

const NOTHING_HELD = { held_reviews: [], open_gates: [] } as unknown as HeldForDecision;

function record(overrides: Partial<Notification> = {}): Notification {
  return {
    id: "n-exact",
    tab_id: "tab-orchestrator",
    workspace_id: "ws-1",
    target_tab_id: TARGET_TAB,
    chat_message_id: TARGET_MESSAGE,
    title: SHARED_TEXT,
    body: null,
    severity: "Info",
    level: "action_needed",
    timestamp: 1_700_000_000_000,
    read: false,
    ...overrides,
  };
}

function legacyRecord(): Notification {
  const legacy = record({ id: "n-legacy", title: "Recorded before deep links", timestamp: 1_699_999_999_000 });
  delete legacy.target_tab_id;
  delete legacy.chat_message_id;
  return legacy;
}

const mounted: Array<{ root: Root; host: HTMLElement }> = [];

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  mounted.push({ root, host });
  await act(async () => root.render(ui));
  return { host, rerender: (next: ReactElement) => act(async () => root.render(next)) };
}

function deepLinks(notifications: Notification[], onSelectTab = vi.fn()): NeedsYouDeepLinks {
  return { notifications, tabs: TABS, workspaces: WORKSPACES, scope: { teamId: TEAM }, onSelectTab };
}

function links(host: HTMLElement): HTMLElement[] {
  return Array.from(host.querySelectorAll<HTMLElement>('[data-testid="needs-you-notification-link"]'));
}

function rowFor(host: HTMLElement, id: string): HTMLElement {
  const row = host.querySelector<HTMLElement>(`[data-notification-id="${id}"]`);
  if (!row) throw new Error(`no Needs you row for ${id}`);
  return row;
}

async function pressEnter(el: HTMLElement) {
  await act(async () => {
    el.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  });
}

afterEach(async () => {
  for (const { root, host } of mounted.splice(0)) {
    await act(async () => root.unmount());
    host.remove();
  }
  act(() => {
    clearChatReveal();
    canvasFocusStore.close();
    canvasFocusStore.closeWorkspace();
  });
  window.localStorage.clear();
  window.sessionStorage.clear();
});

describe("notification deep link identity", () => {
  it("resolves the exact target only when both ids were recorded", () => {
    expect(resolveDeepLink(record(), TABS, WORKSPACES)).toEqual({
      kind: "available",
      tabId: TARGET_TAB,
      workspaceId: "ws-1",
      messageId: TARGET_MESSAGE,
    });
    expect(resolveDeepLink(legacyRecord(), TABS, WORKSPACES)).toEqual({ kind: "unavailable", reason: "no_target" });
    expect(resolveDeepLink(record({ chat_message_id: null }), TABS, WORKSPACES))
      .toEqual({ kind: "unavailable", reason: "no_target" });
  });

  it("reports a closed target tab as unavailable", () => {
    expect(resolveDeepLink(record({ target_tab_id: "tab-closed" }), TABS, WORKSPACES))
      .toEqual({ kind: "unavailable", reason: "tab_gone" });
  });

  it("reports a target tab without a chat view as unavailable", () => {
    expect(resolveDeepLink(record({ target_tab_id: SHELL_TAB }), TABS, WORKSPACES))
      .toEqual({ kind: "unavailable", reason: "no_chat" });
  });

  it("keeps team scope to unread action-needed records asked from inside the team", () => {
    const items = needsYouNotificationItems(
      [
        record(),
        record({ id: "n-foreign", tab_id: "tab-foreign" }),
        record({ id: "n-foreign-target", tab_id: "tab-foreign", target_tab_id: TARGET_TAB }),
        record({ id: "n-read", read: true }),
        record({ id: "n-verbose", level: "verbose" }),
        record({ id: "n-unlevelled", level: undefined }),
      ],
      TABS,
      WORKSPACES,
      { teamId: TEAM },
    );
    expect(items.map((item) => item.id)).toEqual(["n-exact"]);
  });
});

describe("Needs you notification rows", () => {
  it("renders each row as one keyboard-reachable jump with no action buttons", async () => {
    const onSelectTab = vi.fn();
    const { host } = await render(
      <NeedsYouPanel
        held={NOTHING_HELD}
        deepLinks={deepLinks([record(), legacyRecord(), record({ id: "n-closed", target_tab_id: "tab-closed" })], onSelectTab)}
      />,
    );
    const list = host.querySelector("ul");
    expect(list).not.toBeNull();
    expect(list!.querySelectorAll("button, [role='button']")).toHaveLength(0);
    expect(links(host)).toHaveLength(3);
    for (const link of links(host)) expect(link.tabIndex).toBe(0);

    expect(rowFor(host, "n-exact").dataset.deepLink).toBe("available");
    expect(rowFor(host, "n-exact").querySelector('[data-testid="needs-you-notification-unavailable"]')).toBeNull();

    const legacy = rowFor(host, "n-legacy");
    expect(legacy.textContent).toContain("Recorded before deep links");
    expect(legacy.querySelector('[role="link"]')?.getAttribute("aria-disabled")).toBe("true");
    expect(legacy.querySelector('[data-testid="needs-you-notification-unavailable"]')?.textContent)
      .toBe(DEEP_LINK_UNAVAILABLE_COPY.no_target);

    const closed = rowFor(host, "n-closed");
    expect(closed.querySelector('[data-testid="needs-you-notification-unavailable"]')?.textContent)
      .toBe(DEEP_LINK_UNAVAILABLE_COPY.tab_gone);
  });

  it("Enter on an available row selects the target tab and requests its exact message", async () => {
    const onSelectTab = vi.fn();
    const { host } = await render(<NeedsYouPanel held={NOTHING_HELD} deepLinks={deepLinks([record()], onSelectTab)} />);
    await pressEnter(links(host)[0]);

    expect(onSelectTab).toHaveBeenCalledTimes(1);
    expect(onSelectTab).toHaveBeenCalledWith("ws-1", TARGET_TAB);
    expect(currentChatReveal()).toMatchObject({
      notificationId: "n-exact",
      tabId: TARGET_TAB,
      messageId: TARGET_MESSAGE,
      outcome: "pending",
    });
  });

  it("an unavailable row neither selects a tab nor asks for any message", async () => {
    const onSelectTab = vi.fn();
    const { host } = await render(
      <NeedsYouPanel held={NOTHING_HELD} deepLinks={deepLinks([record({ target_tab_id: "tab-closed" })], onSelectTab)} />,
    );
    const [link] = links(host);
    await act(async () => link.click());
    await pressEnter(link);
    expect(onSelectTab).not.toHaveBeenCalled();
    expect(currentChatReveal()).toBeNull();
  });
});

describe("jump from the Board to the chat", () => {
  it("leaves the Board, opens the target tab in Chat, selects it, and the reveal settles in that chat", async () => {
    act(() => {
      canvasFocusStore.openWorkspace({ teamId: TEAM });
      canvasFocusStore.openFocus({ planId: "pl_board" });
    });
    setClientTabUiMode(TARGET_TAB, "terminal");
    const onSelectTab = vi.fn();
    const { host } = await render(<NeedsYouPanel held={NOTHING_HELD} deepLinks={deepLinks([record()], onSelectTab)} />);

    await pressEnter(links(host)[0]);

    expect(canvasFocusStore.isWorkspaceOpen()).toBe(false);
    expect(canvasFocusStore.isOpen()).toBe(false);
    expect(getClientTabUiMode(TARGET_TAB)).toBe("chat");
    expect(resolveInitialTabUiMode(TARGET_TAB, { chatCapable: true })).toBe("chat");
    expect(onSelectTab).toHaveBeenCalledWith("ws-1", TARGET_TAB);
    expect(currentChatReveal()?.outcome).toBe("pending");

    await render(<ChatHarness tabId={TARGET_TAB} ready />);
    expect(currentChatReveal()).toMatchObject({ tabId: TARGET_TAB, messageId: TARGET_MESSAGE, outcome: "revealed" });
  });

  it("a tab without a chat view is unavailable on the Board and a direct jump settles instead of waiting", async () => {
    act(() => canvasFocusStore.openWorkspace({ teamId: TEAM }));
    const onSelectTab = vi.fn();
    const shellRecord = record({ id: "n-shell", target_tab_id: SHELL_TAB });
    const { host } = await render(<NeedsYouPanel held={NOTHING_HELD} deepLinks={deepLinks([shellRecord], onSelectTab)} />);

    expect(rowFor(host, "n-shell").querySelector('[data-testid="needs-you-notification-unavailable"]')?.textContent)
      .toBe(DEEP_LINK_UNAVAILABLE_COPY.no_chat);
    await pressEnter(links(host)[0]);
    expect(onSelectTab).not.toHaveBeenCalled();
    expect(canvasFocusStore.isWorkspaceOpen()).toBe(true);

    act(() => {
      jumpToChatMessage(
        { workspaceId: "ws-1", tabId: SHELL_TAB, messageId: TARGET_MESSAGE, notificationId: "n-shell" },
        TABS[SHELL_TAB],
        onSelectTab,
      );
    });
    expect(currentChatReveal()?.outcome).toBe("no_chat");
    expect(onSelectTab).not.toHaveBeenCalled();
    expect(canvasFocusStore.isWorkspaceOpen()).toBe(true);
  });
});

const TRANSCRIPT = [
  { id: "m-similar", text: SHARED_TEXT },
  { id: TARGET_MESSAGE, text: SHARED_TEXT },
  { id: "m-after", text: "Rolling out now" },
  { id: "m-tail", text: SHARED_TEXT },
];
const ROW_HEIGHT = 100;

function ChatHarness({ tabId, ready }: { tabId: string; ready: boolean }) {
  const { scrollRef, handleScroll, revealMessage } = useChatScroll({
    sessionKey: `${tabId}:deep-link`,
    active: true,
    loading: false,
    messageCount: TRANSCRIPT.length,
    contentVersion: TRANSCRIPT,
    prependVersion: TRANSCRIPT[0].id,
  });
  useChatMessageReveal({ tabId, ready, contentVersion: TRANSCRIPT, revealMessage });
  return (
    <div ref={scrollRef} onScroll={handleScroll} data-testid="chat-scroll">
      {TRANSCRIPT.map((message) => (
        <div key={message.id} data-message-id={message.id}>{message.text}</div>
      ))}
    </div>
  );
}

function stubGeometry(host: HTMLElement): HTMLElement {
  const scroll = host.querySelector<HTMLElement>('[data-testid="chat-scroll"]')!;
  let scrollTop = 0;
  Object.defineProperty(scroll, "clientHeight", { configurable: true, get: () => ROW_HEIGHT });
  Object.defineProperty(scroll, "scrollHeight", { configurable: true, get: () => TRANSCRIPT.length * ROW_HEIGHT });
  Object.defineProperty(scroll, "scrollTop", {
    configurable: true,
    get: () => scrollTop,
    set: (value: number) => { scrollTop = value; },
  });
  scroll.querySelectorAll<HTMLElement>("[data-message-id]").forEach((row, index) => {
    Object.defineProperty(row, "offsetTop", { configurable: true, get: () => index * ROW_HEIGHT });
  });
  return scroll;
}

describe("exact chat message reveal", () => {
  it("scrolls to the row with the exact id, not an earlier message with the same text", async () => {
    const { host } = await render(<ChatHarness tabId={TARGET_TAB} ready />);
    const scroll = stubGeometry(host);
    scroll.scrollTop = 350;

    act(() => { requestChatMessageReveal(TARGET_TAB, TARGET_MESSAGE, "n-exact"); });

    expect(currentChatReveal()?.outcome).toBe("revealed");
    expect(scroll.scrollTop).toBe(TRANSCRIPT.findIndex((message) => message.id === TARGET_MESSAGE) * ROW_HEIGHT);
  });

  it("waits for a ready transcript and leaves another tab's request alone", async () => {
    const { host, rerender } = await render(<ChatHarness tabId={TARGET_TAB} ready={false} />);
    stubGeometry(host);

    act(() => { requestChatMessageReveal("tab-elsewhere", TARGET_MESSAGE, "n-exact"); });
    expect(currentChatReveal()?.outcome).toBe("pending");

    act(() => { requestChatMessageReveal(TARGET_TAB, TARGET_MESSAGE, "n-exact"); });
    expect(currentChatReveal()?.outcome).toBe("pending");

    await rerender(<ChatHarness tabId={TARGET_TAB} ready />);
    expect(currentChatReveal()?.outcome).toBe("revealed");
  });

  it("settles an absent id as missing without moving, and the row turns honestly unavailable", async () => {
    const { host } = await render(<ChatHarness tabId={TARGET_TAB} ready />);
    const scroll = stubGeometry(host);
    scroll.scrollTop = 350;
    const gone = record({ id: "n-gone", chat_message_id: "needs-you-message-gone" });

    act(() => { requestChatMessageReveal(TARGET_TAB, "needs-you-message-gone", gone.id); });

    expect(currentChatReveal()?.outcome).toBe("missing");
    expect(scroll.scrollTop).toBe(350);
    const [item] = needsYouNotificationItems([gone], TABS, WORKSPACES, { teamId: TEAM }, currentChatReveal());
    expect(item.state).toEqual({ kind: "unavailable", reason: "message_gone" });

    const panel = await render(<NeedsYouPanel held={NOTHING_HELD} deepLinks={deepLinks([gone])} />);
    expect(rowFor(panel.host, "n-gone").querySelector('[data-testid="needs-you-notification-unavailable"]')?.textContent)
      .toBe(DEEP_LINK_UNAVAILABLE_COPY.message_gone);
  });
});
