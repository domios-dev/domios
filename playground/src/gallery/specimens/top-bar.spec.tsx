import type { NormalizedChatMessage } from "@codeterm/chat-core";
import "../../../../src/components/Usage/styles.css";
import { TabTopBar, type HeaderTabMenu } from "../../../../src/components/Terminal/TabTopBar";
import { chatChangesStore } from "../../../../src/components/Terminal/chatChangesStore";
import { chatDocumentsStore } from "../../../../src/components/Terminal/chatDocumentsStore";
import type { ChatDocument } from "../../../../src/components/Terminal/chatDocuments";
import { getAgentDescriptor, listAgentDescriptors } from "../../../../src/lib/agentDescriptorCache";
import type { Tab } from "../../../../src/lib/types";
import type { UsageSnapshot } from "../../../../src/types/usage";
import { mockCommand } from "../../mocks/backend";
import { agentTab, CODEX, ProductProviders, WORKSPACE_ID } from "../fixtures";
import { PinnedMenu } from "../stageTools";
import { defineSpecimen } from "../types";

const HOUR = 3_600_000;
const REPO = "/Developer/codeterm";

mockCommand("plugin_manager_snapshot", () => ({
  plugins: [{ id: "git", display_name: "Git", capabilities: { status_bubble: true } }],
  channels: [],
  diff: [],
  marketplace: [],
}));
mockCommand("plugin_bubble", () => ({ label: "master", tone: "accent", tooltip: "Git · master · clean", action: "openPanel:git" }));
mockCommand("repo_watch_subscribe", (args) => ({ status: "watching", token: `watch-${String(args?.cwd)}`, identity: String(args?.cwd) }));
mockCommand("check_local_preview_files", (args) => ({ paths: (args?.paths as string[]) ?? [], home: "/Users/andrey" }));

function usage(provider: string, session: number, weekly: number): UsageSnapshot {
  const now = Date.now();
  return {
    billing_mode: "subscription", limits_reported: true, id: 1, provider, account_id: null, captured_at: now, parsed_ok: true,
    session_pct: session, session_resets_at: now + 2 * HOUR, weekly_pct: weekly, weekly_resets_at: now + 72 * HOUR,
    spend_cents: null, spend_limit_cents: null, spend_remaining_cents: null, spend_resets_at: null, failure_reason: null, probe_command: null,
  };
}

const USAGE: Record<string, UsageSnapshot> = {
  claude: usage("claude", 34, 12),
  codex: usage("codex", 71, 48),
  "gallery-usage-danger": usage("gallery-usage-danger", 93, 64),
};
mockCommand("usage_latest", (args) => USAGE[String(args?.provider)] ?? null);

const ids = { all: "bar-all", warn: "bar-warn", danger: "bar-danger", both: "bar-both", changes: "bar-changes", menu: "bar-menu" };

function documents(...names: string[]): ChatDocument[] {
  return names.map((name, index) => ({ path: `${REPO}/docs/${name}`, kind: name.endsWith(".png") ? "image" : name.endsWith(".html") ? "page" : "document", name, shownAt: Date.now() - index * 60_000 }));
}

const lines = (count: number, prefix: string) => Array.from({ length: count }, (_, index) => `${prefix} ${index}`).join("\n");

function edits(turn: string, files: Array<[path: string, removed: number, added: number]>): NormalizedChatMessage[] {
  return [
    { id: `${turn}-user`, type: "user", content: "Restyle the top bar" },
    ...files.map(([path, removed, added], index): NormalizedChatMessage => ({
      id: `${turn}-edit-${index}`, type: "tool_result", content: "", toolId: `${turn}-tool-${index}`, toolError: false,
      toolEdits: [{ path: `${REPO}/${path}`, old: lines(removed, "old"), new: lines(added, "new") }],
    })),
  ];
}

const CHANGES = edits("turn", [["src/components/Terminal/TabTopBar.tsx", 21, 64], ["src/components/Terminal/tabBarPill.css", 9, 48]]);
for (const tabId of [ids.all, ids.both, ids.changes, ids.menu]) chatChangesStore.set(tabId, CHANGES);
for (const tabId of [ids.all, ids.both, ids.menu]) chatDocumentsStore.set(tabId, documents("top-bar.md", "pill-board.png", "glass-study.html"));

const context = (used: number) => ({ used_tokens: used, window_tokens: 200_000, cost_usd: null, source: "transcript" });

function tab(id: string, overrides: Partial<Tab> = {}): Tab {
  return agentTab(id, { title: "Top bar restyle", cwd: REPO, context: context(84_000), ...overrides });
}

function headerMenu(menuTab: Tab): HeaderTabMenu {
  return {
    tab: menuTab,
    context: { status: "Idle", descriptor: getAgentDescriptor(menuTab.agent_type), descriptors: listAgentDescriptors(), activity: null, orchestrators: [] },
    onRename: () => undefined,
    onClose: () => undefined,
  };
}

type TopBarSpecimenProps = { tab: Tab; usageProvider?: string | null; menu?: boolean };

function TopBarSpecimen({ tab: barTab, usageProvider = barTab.agent_type ?? null, menu = false }: TopBarSpecimenProps) {
  const bar = <div className="gallery-fill" data-tab-id={barTab.id}>
    <TabTopBar
      tabId={barTab.id}
      shellTab={barTab}
      cwd={barTab.cwd ?? null}
      workspaceId={WORKSPACE_ID}
      showSwitch
      usageProvider={usageProvider}
      usageVisible={Boolean(usageProvider)}
      usageModelId={barTab.agent_session_model ?? null}
      tabMenu={headerMenu(barTab)}
      isActive
    />
  </div>;
  return <ProductProviders tabs={[barTab]}>
    {menu ? <PinnedMenu trigger='[aria-label="Tab actions"]'>{bar}</PinnedMenu> : bar}
  </ProductProviders>;
}

export default defineSpecimen({
  id: "top-bar",
  title: "Top bar",
  category: "Chrome",
  where: "src/components/Terminal/TabTopBar.tsx",
  stage: "full",
  notes: "The shipped TabTopBar for a chat-capable agent tab: Chat | Terminal switch, the Git plugin bubble, UsageBubble, ComposerContextMeter, the documents + changes pill group and the kebab TabActionsMenu, all on the shared ct-bar-pill. Usage, plugin, repository-watch and document checks come from mocked IPC; documents and changes are seeded into their product stores.",
  Component: TopBarSpecimen,
  states: [
    { id: "all", label: "Chat tab · every pill", props: { tab: tab(ids.all) } },
    { id: "usage-warn", label: "Usage warn · session 71%", props: { tab: tab(ids.warn, { agent_type: CODEX.id, agent_session_model: "gpt-6-sol", context: context(131_000) }) } },
    { id: "usage-danger", label: "Usage danger · session 93%", props: { tab: tab(ids.danger, { context: context(188_000) }), usageProvider: "gallery-usage-danger" } },
    { id: "documents-changes", label: "Documents + changes group", props: { tab: tab(ids.both, { context: undefined }), usageProvider: null } },
    { id: "changes-only", label: "Changes only", props: { tab: tab(ids.changes, { context: undefined }), usageProvider: null } },
    { id: "menu", label: "Kebab menu open", props: { tab: tab(ids.menu), menu: true } },
  ],
});
