import type { ComponentProps } from "react";
import { TabItem } from "../../../../src/components/Sidebar/TabItem";
import type { Tab } from "../../../../src/lib/types";
import { agentTab, CODEX, ProductProviders, shellTab, WORKSPACE_ID } from "../fixtures";
import { ForcePseudo, PinnedMenu } from "../stageTools";
import { defineSpecimen } from "../types";

type RowProps = Partial<Omit<ComponentProps<typeof TabItem>, "tab">> & { tab: Tab };
type TabRowSpecimenProps = { rows: RowProps[]; hover?: boolean; menu?: "open" | "model" };

const noop = () => undefined;

function Row(props: RowProps) {
  return <TabItem index={0} isActive={false} onClick={noop} onClose={noop} canClose workspaceId={WORKSPACE_ID} hasAgent={Boolean(props.tab.agent_type)} {...props} />;
}

function TabRowSpecimen({ rows, hover = false, menu }: TabRowSpecimenProps) {
  const list = <div className="gallery-sidebar">{rows.map((row) => <Row key={row.tab.id} {...row} />)}</div>;
  return <ProductProviders tabs={rows.map((row) => row.tab)}>
    {menu
      ? <PinnedMenu trigger='[aria-label="Tab actions"]' submenu={menu === "model" ? "Model" : undefined}>{list}</PinnedMenu>
      : hover ? <ForcePseudo selector=".sidebar-tab" state="hover">{list}</ForcePseudo> : list}
  </ProductProviders>;
}

const working = { state: "working" as const, status_line: "Rebuilding the gallery stages… (4m 12s · ↓ 18.2k tokens)" };
const orchestrator = agentTab("row-orchestrator", { title: "Motion Design Lead", role: "Manager", role_profile: "orchestrator", can_spawn: true, agent_state: working });
const planner = agentTab("row-planner", { title: "Release planner", role: "Manager", role_profile: "planner", agent_session_model: "fable" });
const worker = agentTab("row-worker", { title: "Gallery worker", role: "Worker", role_profile: "worker", parent_tab_id: orchestrator.id, depth: 1, agent_type: CODEX.id, agent_session_model: "gpt-6-sol", agent_state: working });

export default defineSpecimen({
  id: "sidebar-tab-row",
  title: "Sidebar tab row",
  category: "Sidebar",
  where: "src/components/Sidebar/TabItem.tsx",
  stage: "wide",
  notes: "The shipped TabItem row with real provider descriptors from the plugin manifests. The kebab, hover and mesh-dot states use the product CSS; menu stages open the real TabMenu programmatically and keep it pinned.",
  Component: TabRowSpecimen,
  states: [
    { id: "working", label: "Agent working", props: { rows: [{ tab: agentTab("row-working", { title: "Refactor composer", agent_state: working }), status: "Working" }] } },
    { id: "working-selected", label: "Working · selected", props: { rows: [{ tab: agentTab("row-working-selected", { title: "Refactor composer", agent_state: working }), status: "Working", isActive: true }] } },
    { id: "idle", label: "Idle · selected vs not", props: { rows: [{ tab: agentTab("row-idle-a", { title: "Docs sync" }), isActive: true }, { tab: agentTab("row-idle-b", { title: "Changelog draft", agent_type: CODEX.id, agent_session_model: "gpt-6-sol" }) }] } },
    { id: "waiting", label: "Waiting on you", props: { rows: [{ tab: agentTab("row-waiting", { title: "Deploy check", agent_state: { state: "question", subtype: "permission" } }), status: "Waiting" }] } },
    { id: "failed", label: "Launch failed", props: { rows: [{ tab: agentTab("row-failed", { title: "Nightly triage", boot_stage: "spawn_failed", boot_failure_reason: "provider exited with code 1" }), status: "Error" }] } },
    { id: "roles", label: "Orchestrator · planner · worker", props: { rows: [{ tab: orchestrator, status: "Working" }, { tab: planner }, { tab: worker, status: "Working", reportCount: 3 }] } },
    { id: "general-agent", label: "General Agent", props: { rows: [{ tab: agentTab("row-general", { title: "General", role: "Manager", role_profile: "general_agent", protected: true, description: "Planning the 1.14 release" }), isActive: true }] } },
    {
      id: "shells", label: "Utility shells · purpose icons", props: {
        rows: [
          { tab: shellTab("row-shell-build", { title: "pnpm build", purpose: "build", description: "Production bundle", shell_command_state: { activity: "running", last_exit_code: null } }), status: "Working", startedBy: { id: orchestrator.id, label: "Motion Design Lead", onOpen: noop } },
          { tab: shellTab("row-shell-dev", { title: "vite", purpose: "dev", description: "Playground dev server", shell_command_state: { activity: "idle", last_exit_code: 0 } }), startedBy: { id: "closed-tab", label: "Closed planner" } },
          { tab: shellTab("row-shell-qa", { title: "playwright", purpose: "qa", shell_command_state: { activity: "idle", last_exit_code: 1 } }) },
          { tab: shellTab("row-shell-plain", { title: "zsh" }) },
        ],
      },
    },
    { id: "mesh", label: "Mesh tab · dot at the row end", props: { rows: [{ tab: agentTab("mesh:peer-a/row-mesh", { title: "Render farm", remote_peer_id: "peer-a", remote_peer_name: "blackstar", remote_tab_id: "row-mesh", role: "Manager", role_profile: "orchestrator", ownership: { controller_title: "Mac mini", peer_id: "peer-a", peer_name: "blackstar" } }) }, { tab: agentTab("row-local", { title: "Local agent" }) }] } },
    { id: "managed", label: "Managed tab", props: { rows: [{ tab: agentTab("row-managed", { title: "Spec review", managed: { manager_tab_id: orchestrator.id, manager_name: "Motion Design Lead", manager_closed: false } }) }] } },
    {
      id: "attachment", label: "Surface attachment under its owner", props: {
        rows: [
          { tab: agentTab("row-owner", { title: "Docs agent" }), attachmentToggle: { count: 2, collapsed: false, onToggle: noop } },
          { tab: { id: "row-surface", title: "Board", tab_type: "Terminal", pty_id: null, cwd: null, parent_tab_id: "row-owner", backing: { kind: "surface", plugin_id: "dash", surface_id: "board", instance_id: "inst-1", owner_label: true } }, variant: "attachment", insetPx: 30, ownerTitle: "Docs agent" },
        ],
      },
    },
    { id: "indicators", label: "Telegram · auto-confirm · reports", props: { rows: [{ tab: agentTab("row-indicators", { title: "Inbox agent", connected_services: ["telegram"], permission_auto_confirm: true }), reportCount: 2 }] } },
    { id: "hover", label: "Hover · kebab revealed", props: { rows: [{ tab: agentTab("row-hover", { title: "Refactor composer" }) }], hover: true } },
    { id: "menu", label: "Kebab menu open", props: { rows: [{ tab: agentTab("row-menu", { title: "Refactor composer" }) }], menu: "open" } },
    { id: "menu-model", label: "Model submenu open", props: { rows: [{ tab: agentTab("row-menu-model", { title: "Refactor composer" }) }], menu: "model" } },
  ],
});
