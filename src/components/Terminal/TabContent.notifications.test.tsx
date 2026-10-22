import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Notification, Tab, ThemeConfig } from "../../lib/types";
import { reducer } from "../../store/multiplexer/reducer";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { Titlebar } from "../Titlebar/Titlebar";
import { TabContent } from "./TabContent";

const fixture = vi.hoisted(() => ({ state: null as ExtendedState | null }));
vi.mock("../Titlebar/KeepAwakeTray", () => ({ KeepAwakeTray: () => null }));
vi.mock("../WebPeerPicker/WebPeerPicker", () => ({ WebPeerPicker: () => null }));
vi.mock("@codeterm/chat-ui", () => ({ LocalArtifactScope: ({ children }: { children: React.ReactNode }) => <>{children}</>, RichMarkdown: ({ text }: { text: string }) => <div>{text}</div> }));
vi.mock("../../lib/useAgentDescriptor", () => ({ useAgentDescriptor: () => ({ supports_chat_ui: true, settings_schema: [] }) }));
vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [], loading: false, error: null }) }));
vi.mock("./useHeaderTabMenu", () => ({ useHeaderTabMenu: () => null }));
vi.mock("../MemoryPanel/workerFocusStore", () => ({ isTabVisible: () => true }));
vi.mock("../AgentPicker", () => ({ isFirstRunGeneralTab: () => false }));
vi.mock("./TerminalTab", () => ({ TerminalTab: () => <main aria-label="Terminal" /> }));
vi.mock("./TabSessionChat", () => ({ TabSessionChat: () => <main aria-label="Chat" /> }));
vi.mock("./TabTopBar", () => ({ TabTopBar: () => <nav aria-label="Tab controls" /> }));
vi.mock("./RestoredShell", () => ({ RestoredShell: () => null }));
vi.mock("./SurfaceTabHost", () => ({ SurfaceTabHost: () => <main aria-label="Plugin surface" /> }));
vi.mock("../../store/multiplexer", () => ({
  useMuxSelector: (select: (state: unknown) => unknown) => select(fixture.state),
  useMultiplexer: () => ({ markRead: vi.fn(), answerNotification: vi.fn() }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const host = document.createElement("div");
const root = createRoot(host);
beforeEach(() => {
  vi.stubGlobal("__CODETERM_DEV__", false);
  vi.stubGlobal("__APP_VERSION__", "test");
  fixture.state = {
    workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null,
    notifications: [], toasts: [], tabStatuses: {}, activeTabIds: {}, generation: 0, pendingCloses: [],
  };
  document.body.append(host);
});
afterEach(async () => {
  await act(async () => root.render(null));
  host.remove();
  vi.unstubAllGlobals();
});

it.each([false, true])("renders tab controls and content without an attention region (chat=%s)", async (chat) => {
  const tab = { id: "agent", title: "tab-token", pty_id: null, tab_type: "Terminal", cwd: null,
    backing: chat ? { kind: "chat_backend", plugin_id: "fixture" } : undefined } as Tab;
  await act(async () => root.render(<TabContent tab={tab} isFocused isVisible
    onFocusTab={() => undefined} onPtySpawned={() => undefined} theme={{ background: "black" } as ThemeConfig} />));
  expect(host.querySelector('nav[aria-label="Tab controls"]')).not.toBeNull();
  expect(host.querySelector(`main[aria-label="${chat ? "Chat" : "Terminal"}"]`)).not.toBeNull();
  expect(host.querySelector('[role="region"]')).toBeNull();
  expect(host.querySelector('button[aria-label^="Dismiss attention"]')).toBeNull();
});

it.each([false, true])("keeps an agent choice in the bell without interrupting tab content (chat=%s)", async (chat) => {
  const notification: Notification = {
    id: "agent-choice", tab_id: "agent", workspace_id: null, title: "question-token", body: "detail-token",
    timestamp: 1, severity: "Warning", level: "action_needed", read: false,
    actions: [{ type: "answer", label: "option-a" }, { type: "answer", label: "option-b" }],
    answer: { asked_from_tab: "agent", answered: null, delivered: false },
  };
  const tab = { id: "agent", title: "tab-token", pty_id: null, tab_type: "Terminal", cwd: null,
    backing: chat ? { kind: "chat_backend", plugin_id: "fixture" } : undefined } as Tab;
  const answer = vi.fn((id: string, label: string) => {
    fixture.state = reducer(fixture.state!, { type: "ANSWER_NOTIFICATION", id, answered: label, delivered: true });
  });
  const render = () => act(async () => root.render(<>
    <Titlebar theme={{} as ThemeConfig} notifications={fixture.state!.notifications} toasts={fixture.state!.toasts}
      onDismissToast={vi.fn()} onMarkNotificationRead={vi.fn()} onMarkAllNotificationsRead={vi.fn()}
      onDeleteNotification={vi.fn()} onClearNotifications={vi.fn()} onNavigateToast={vi.fn()} onAnswerNotification={answer} />
    <TabContent tab={tab} isFocused isVisible onFocusTab={vi.fn()} onPtySpawned={vi.fn()}
      theme={{ background: "black" } as ThemeConfig} />
  </>));
  await render();
  const controls = host.querySelector('nav[aria-label="Tab controls"]');
  const content = host.querySelector(`main[aria-label="${chat ? "Chat" : "Terminal"}"]`);
  fixture.state = reducer(fixture.state!, { type: "ADD_NOTIFICATION", notification });
  await render();
  expect(fixture.state.notifications).toHaveLength(1);
  expect(fixture.state.toasts).toHaveLength(0);
  expect(host.querySelector('nav[aria-label="Tab controls"]')).toBe(controls);
  expect(host.querySelector(`main[aria-label="${chat ? "Chat" : "Terminal"}"]`)).toBe(content);
  expect(host.querySelectorAll('[role="region"], [role="alert"], [role="dialog"]')).toHaveLength(0);
  const bell = host.querySelector<HTMLButtonElement>('button[aria-label="Notifications"]')!;
  const unread = host.querySelector('[role="status"]')!;
  expect(host.querySelectorAll('[role="status"]')).toHaveLength(1);
  expect(bell.getAttribute("aria-describedby")).toBe(unread.id);
  expect(unread.textContent).not.toBe("");
  await act(async () => bell.click());
  const details = document.querySelectorAll('[role="region"][aria-label="Message from question-token"]');
  expect(details).toHaveLength(1);
  expect(host.contains(details[0])).toBe(false);
  const choices = [...document.querySelectorAll<HTMLButtonElement>("button")]
    .filter((button) => notification.actions!.some((action) => button.textContent === action.label));
  expect(choices).toHaveLength(2);
  await act(async () => choices[1].click());
  expect(answer).toHaveBeenCalledExactlyOnceWith(notification.id, "option-b");
  await render();
  expect(fixture.state.notifications[0].answer).toMatchObject({ answered: "option-b", delivered: true });
  expect([...document.querySelectorAll("button")].some((button) => button.textContent === "option-a" || button.textContent === "option-b")).toBe(false);
  expect(host.querySelectorAll('[role="region"], [role="alert"], [role="dialog"]')).toHaveLength(0);
});
