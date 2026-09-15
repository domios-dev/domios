import type { ReactNode, RefObject } from "react";
import {
  Bot,
  Check,
  Copy,
  Cpu,
  MessageSquare,
  MessageSquareX,
  Pencil,
  Puzzle,
  RefreshCw,
  RotateCcw,
  ShieldCheck,
  UserCog,
  X,
  type LucideIcon,
} from "lucide-react";
import { DropdownMenu, type DropdownMenuEntry } from "../shared/DropdownMenu";
import { PluginIcon } from "../Extensions/PluginIcon";
import { TAB_SESSION_LABELS } from "./tabSessionActions";
import {
  TAB_MENU_LABELS,
  type TabMenuAction,
  type TabMenuEntry,
  type TabMenuGlyph,
  type TabMenuHeaderEntry,
  type TabMenuIcon,
  type TabMenuLeafEntry,
  type TabMenuSubmenuEntry,
} from "./tabMenuModel";

const GLYPHS: Record<TabMenuGlyph, LucideIcon> = {
  rename: Pencil,
  role: UserCog,
  model: Cpu,
  provider: Bot,
  reconnect: RefreshCw,
  restart: RotateCcw,
  autoConfirm: ShieldCheck,
  plugins: Puzzle,
  telegram: MessageSquare,
  telegramClose: MessageSquareX,
  close: X,
};

type ActionHandler = (action: TabMenuAction) => void;

function menuIcon(icon: TabMenuIcon): ReactNode {
  if (icon === null) return undefined;
  if (typeof icon === "string") {
    const Glyph = GLYPHS[icon];
    return <Glyph size={11} strokeWidth={2} />;
  }
  return <PluginIcon plugin={{ icon_svg: icon.svg, icon_color: icon.color }} size={11} />;
}

function bind(action: TabMenuAction | null, onAction: ActionHandler): (() => void) | undefined {
  return action ? () => onAction(action) : undefined;
}

function leafEntry(entry: TabMenuLeafEntry, onAction: ActionHandler): DropdownMenuEntry {
  switch (entry.kind) {
    case "separator":
      return { separator: true, key: entry.id };
    case "check":
      return {
        label: entry.label,
        icon: menuIcon(entry.icon),
        checked: entry.checked,
        description: entry.description,
        keepOpen: entry.keepOpen,
        onClick: bind(entry.action, onAction),
        "data-testid": entry.testId,
      };
    case "item":
      return {
        label: entry.label,
        icon: entry.selected === undefined
          ? menuIcon(entry.icon)
          : entry.selected ? <Check size={11} strokeWidth={2} /> : <span className="w-3 h-3" />,
        selected: entry.selected,
        disabled: entry.disabled,
        unavailable: entry.unavailable,
        title: entry.title,
        description: entry.hint,
        danger: entry.danger,
        shortcut: entry.shortcut,
        onClick: bind(entry.action, onAction),
        "data-testid": entry.testId,
      };
  }
}

function submenuEntry(entry: TabMenuSubmenuEntry, onAction: ActionHandler): DropdownMenuEntry {
  return {
    label: entry.label,
    icon: menuIcon(entry.icon),
    disabled: entry.disabled,
    title: entry.title,
    "data-testid": entry.testId,
    submenu: {
      label: entry.label,
      description: entry.description,
      items: entry.entries.map((child) => leafEntry(child, onAction)),
    },
  };
}

function TabMenuSessionHeader({ entry, onAction }: { entry: TabMenuHeaderEntry; onAction: ActionHandler }) {
  const { sessionId, copyAction } = entry;
  return (
    <div className="mx-1 mb-1 flex min-w-48 items-center gap-1 border-b border-[var(--ct-border)] px-2 pb-1.5 pt-0.5">
      <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-[var(--ct-muted-fg)]" title={sessionId ?? "Session ID is not available yet"}>
        {sessionId ?? "ID not available yet"}
      </span>
      <button
        type="button"
        role="menuitem"
        data-testid={entry.testId}
        aria-label={TAB_SESSION_LABELS.copy}
        title={TAB_SESSION_LABELS.copy}
        disabled={!copyAction}
        onClick={(event) => {
          event.stopPropagation();
          if (copyAction) onAction(copyAction);
        }}
        className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] disabled:cursor-not-allowed disabled:opacity-35"
      >
        <Copy size={12} />
      </button>
    </div>
  );
}

interface TabMenuProps {
  entries: TabMenuEntry[];
  onAction: ActionHandler;
  trigger: ReactNode;
  className?: string;
  menuTestId?: string;
  contextMenuTargetRef?: RefObject<HTMLElement | null>;
  onOpenChange?: (open: boolean) => void;
}

export function TabMenu({ entries, onAction, ...menu }: TabMenuProps) {
  const header = entries.find((entry): entry is TabMenuHeaderEntry => entry.kind === "header");
  const items = entries.flatMap((entry): DropdownMenuEntry[] => {
    if (entry.kind === "header") return [];
    return [entry.kind === "submenu" ? submenuEntry(entry, onAction) : leafEntry(entry, onAction)];
  });
  return (
    <DropdownMenu
      {...menu}
      items={items}
      menuLabel={TAB_MENU_LABELS.menu}
      header={header ? <TabMenuSessionHeader entry={header} onAction={onAction} /> : undefined}
    />
  );
}
