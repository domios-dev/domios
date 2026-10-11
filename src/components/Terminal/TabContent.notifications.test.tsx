import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { Tab, ThemeConfig } from "../../lib/types";
import { TabContent } from "./TabContent";

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
  useMuxSelector: (select: (state: unknown) => unknown) => select({
    notifications: [{ id: "ask", tab_id: "agent", title: "question-token", body: "detail-token", level: "action_needed", read: false }],
    tabStatuses: {},
  }),
  useMultiplexer: () => ({ markRead: vi.fn(), answerNotification: vi.fn() }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const host = document.createElement("div");
const root = createRoot(host);
afterEach(async () => { await act(async () => root.render(null)); });

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
