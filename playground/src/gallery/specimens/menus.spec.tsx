import { useMemo, useState, type ReactNode } from "react";
import { Copy, FolderOpen, Pencil, Pin, Share2, Trash2 } from "lucide-react";
import { DropdownMenu, DropdownSelect, type DropdownMenuEntry } from "../../../../src/components/shared/DropdownMenu";
import { RowMenuButton } from "../../../../src/components/shared/RowMenuButton";
import { TabMenu } from "../../../../src/components/Terminal/TabMenu";
import { buildTabMenu } from "../../../../src/components/Terminal/tabMenuModel";
import { listAgentDescriptors, getAgentDescriptor } from "../../../../src/lib/agentDescriptorCache";
import { usePluginTabMenu } from "../../../../src/lib/pluginTabMenu";
import { ProviderGlyph } from "../../../../src/components/ProviderGlyph";
import { agentTab, CLAUDE, CODEX } from "../fixtures";
import { PinnedMenu } from "../stageTools";
import { defineSpecimen } from "../types";

const noop = () => undefined;
const ICON = { size: 14, strokeWidth: 1.9 } as const;

const ROOT: DropdownMenuEntry[] = [
  { label: "Rename", icon: <Pencil {...ICON} />, shortcut: "F2", onClick: noop },
  { label: "Duplicate", icon: <Copy {...ICON} />, onClick: noop },
  { label: "Reveal in folder", icon: <FolderOpen {...ICON} />, onClick: noop },
  { label: "Share", icon: <Share2 {...ICON} />, submenu: { label: "Share", description: "Send this tab to another place", items: [
    { label: "Copy link", onClick: noop },
    { label: "Send to mesh peer", description: "blackstar · online", onClick: noop },
    { label: "Send to Telegram", disabled: true, title: "Telegram bridge is not connected", onClick: noop },
  ] } },
  { separator: true, key: "danger" },
  { label: "Close tab", icon: <Trash2 {...ICON} />, danger: true, onClick: noop },
];

const STATES: DropdownMenuEntry[] = [
  { label: "Pinned", icon: <Pin {...ICON} />, checked: true, keepOpen: true, onClick: noop },
  { label: "Follow output", checked: false, keepOpen: true, onClick: noop },
  { separator: true, key: "choice" },
  { label: "Compact", selected: false, onClick: noop },
  { label: "Comfortable", selected: true, onClick: noop },
  { separator: true, key: "inert" },
  { label: "Reconnect", disabled: true, title: "Only a live tab can reconnect", onClick: noop },
  { label: "Restart session", unavailable: true, description: "The provider has no resumable session yet", onClick: noop },
  { label: "Opus 5.5", badge: "new", description: "Best for everyday, complex tasks.", onClick: noop },
];

const GROUPS: DropdownMenuEntry[] = [
  { label: "Claude Code", items: CLAUDE.available_models.slice(0, 3).map((model, index) => ({ label: model.display_name, selected: index === 0, onClick: noop })) },
  { label: "Codex", items: CODEX.available_models.slice(0, 3).map((model) => ({ label: model.display_name, selected: false, onClick: noop })) },
];

function Trigger({ label = "Open menu" }: { label?: string }) {
  return <RowMenuButton label={label} touchMode={false} />;
}

function MenuStage({ items, submenu, header }: { items: DropdownMenuEntry[]; submenu?: string; header?: ReactNode }) {
  return <PinnedMenu trigger='[aria-label="Open menu"]' submenu={submenu}>
    <div className="gallery-row"><DropdownMenu items={items} header={header} trigger={<Trigger />} /></div>
  </PinnedMenu>;
}

function SelectStage() {
  const [value, setValue] = useState("opus");
  const options = CLAUDE.available_models.map((model) => ({ value: model.id, label: model.display_name, group: model.group ?? undefined }));
  return <PinnedMenu trigger='[data-testid="gallery-select"]'>
    <div style={{ width: 220 }}><DropdownSelect options={options} value={value} onChange={setValue} prefix="Model" data-testid="gallery-select" /></div>
  </PinnedMenu>;
}

function TabMenuStage({ submenu }: { submenu?: string }) {
  const tab = useMemo(() => agentTab("menu-tab", { title: "Refactor composer" }), []);
  const pluginTabMenu = usePluginTabMenu();
  const entries = useMemo(() => buildTabMenu({
    tab, surface: "sidebar", status: "Idle", descriptor: getAgentDescriptor(tab.agent_type), descriptors: listAgentDescriptors(),
    activity: null, orchestrators: [], canRename: true, canClose: true, pluginTabMenu, telegramThread: null,
  }), [pluginTabMenu, tab]);
  const icon = <ProviderGlyph iconSvg={CLAUDE.icon_svg} iconColor={CLAUDE.icon_color} className="leading-none text-[16px]" />;
  return <PinnedMenu trigger='[aria-label="Open menu"]' submenu={submenu}>
    <div className="gallery-row">
      <TabMenu entries={entries} onAction={noop} trigger={<Trigger />} identity={{ icon, title: tab.title, subtitle: "Claude Code · opus" }} />
    </div>
  </PinnedMenu>;
}

type MenuSpecimenProps = { kind: "root" | "submenu" | "states" | "groups" | "select" | "tab" | "tab-provider" };

function MenuSpecimen({ kind }: MenuSpecimenProps) {
  if (kind === "root") return <MenuStage items={ROOT} />;
  if (kind === "submenu") return <MenuStage items={ROOT} submenu="Share" />;
  if (kind === "states") return <MenuStage items={STATES} />;
  if (kind === "groups") return <MenuStage items={GROUPS} />;
  if (kind === "select") return <SelectStage />;
  return <TabMenuStage submenu={kind === "tab-provider" ? "Provider" : undefined} />;
}

export default defineSpecimen({
  id: "menus",
  title: "Dropdown menus",
  category: "Menus",
  where: "src/components/shared/DropdownMenu.tsx · src/components/Terminal/TabMenu.tsx",
  stage: "wide",
  notes: "The shipped DropdownMenu, DropdownSelect and TabMenu, opened through their real triggers and pinned. A submenu pages in place at the root menu's width. The team menu lives on the Team unit specimen.",
  Component: MenuSpecimen,
  states: [
    { id: "root", label: "Root · icons, shortcut, separator, danger", props: { kind: "root" } },
    { id: "submenu", label: "Submenu open · same width", props: { kind: "submenu" } },
    { id: "states", label: "Checked · selected · disabled · unavailable · badge", props: { kind: "states" } },
    { id: "groups", label: "Group headers", props: { kind: "groups" } },
    { id: "select", label: "DropdownSelect · grouped options", props: { kind: "select" } },
    { id: "tab", label: "TabMenu · identity head + session id", props: { kind: "tab" } },
    { id: "tab-provider", label: "TabMenu · Provider submenu", props: { kind: "tab-provider" } },
  ],
});
