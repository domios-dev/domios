import { createRoot } from "react-dom/client";
import "../../../src/styles/globals.css";
import { TabItem } from "../../../src/components/Sidebar/TabItem";
import { SidebarCompactRail } from "../../../src/components/Sidebar/SidebarCompactRail";
import { DEFAULT_THEME, themeToCSS } from "../../../src/lib/themeCss";
import type { Tab, ThemeConfig } from "../../../src/lib/types";
import lightTheme from "../../../src-tauri/themes/github-light.toml?raw";

const light = new URLSearchParams(location.search).get("theme") === "light";
const theme = light ? Object.fromEntries(
  [...lightTheme.matchAll(/^(\w+)\s*=\s*"([^"]+)"/gm)].map(([, key, value]) => [key, value]),
) as unknown as ThemeConfig : DEFAULT_THEME;
for (const [key, value] of Object.entries(themeToCSS(theme))) document.documentElement.style.setProperty(key, value);
document.body.style.cssText = "margin:0;background:var(--ct-bg);color:var(--ct-fg)";
const tab: Tab = {
  id: "starting", title: "Elgar", agent_type: "fixture-agent", tab_type: "Terminal",
  pty_id: null, cwd: null, boot_stage: "agent_launching", agent_state: { state: "working" },
  description: "Preparing the agent session",
};

createRoot(document.getElementById("root")!).render(
  <main style={{ padding: 24, width: 360 }} aria-label={`${light ? "Light" : "Dark"} sidebar starting fixture`}>
    <TabItem tab={tab} status="Working" hasAgent index={0} isActive={false}
      onClick={() => undefined} onClose={() => undefined} canClose={false} workspaceId="fixture" />
    <SidebarCompactRail groups={[{ workspaceId: "fixture", label: "Workspace", tabs: [tab], activeTabId: null }]}
      tabStatuses={{ starting: "Working" }} onActivate={() => undefined} />
  </main>,
);
