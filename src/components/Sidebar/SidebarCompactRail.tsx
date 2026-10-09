import type { Tab, TabStatus } from "../../lib/types";
import { tabActivity } from "../../lib/tabActivity";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { AgentIcon } from "../AgentIcon";
import { TerminalIcon } from "../Icons";

export interface CompactRailGroup {
  workspaceId: string;
  label: string;
  tabs: Tab[];
  activeTabId: string | null;
}

interface Props {
  groups: CompactRailGroup[];
  tabStatuses: Record<string, TabStatus>;
  onActivate: (workspaceId: string, tabId: string) => void;
}

function monogram(label: string): string {
  const words = label.trim().split(/[\s_-]+/).filter(Boolean);
  if (words.length > 1) return `${words[0][0]}${words[1][0]}`.toUpperCase();
  return label.trim().slice(0, 2).toUpperCase();
}

/** Narrowest useful sidebar: General agents and the selected workspace's orchestrators only. */
export function SidebarCompactRail({ groups, tabStatuses, onActivate }: Props) {
  const visible = groups.filter((group) => group.tabs.length > 0);
  return (
    <nav className="ct-compact-rail" aria-label="Main agents" data-testid="sidebar-compact-rail">
      {visible.map((group) => (
        <div key={group.workspaceId} className="ct-compact-rail__group" role="group" aria-label={group.label}>
          <span className="ct-compact-rail__label" title={group.label} aria-hidden="true">{monogram(group.label)}</span>
          {group.tabs.map((tab) => {
            const title = displayTitleForTab(tab);
            const activity = tabActivity(tab, tabStatuses[tab.id] ?? null).kind;
            const active = tab.id === group.activeTabId;
            return (
              <button
                key={tab.id}
                type="button"
                className="ct-compact-rail__tab"
                data-active={active || undefined}
                data-activity={activity}
                aria-current={active ? "page" : undefined}
                aria-label={`Open tab ${title}`}
                title={title}
                onClick={() => onActivate(group.workspaceId, tab.id)}
              >
                {tab.agent_type ? <AgentIcon providerId={tab.agent_type} className="text-[15px]" /> : <TerminalIcon size={14} />}
                <span className="ct-compact-rail__status" aria-hidden="true" />
              </button>
            );
          })}
        </div>
      ))}
    </nav>
  );
}
