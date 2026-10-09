import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { Tab, TabStatus } from "../../lib/types";
import { tabActivity } from "../../lib/tabActivity";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { AgentIcon } from "../AgentIcon";
import { TerminalIcon } from "../Icons";
import { TabItem } from "./TabItem";

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

interface Peek {
  tab: Tab;
  workspaceId: string;
  active: boolean;
  top: number;
  left: number;
}

const PEEK_RELEASE_MS = 140;

function monogram(label: string): string {
  const words = label.trim().split(/[\s_-]+/).filter(Boolean);
  if (words.length > 1) return `${words[0][0]}${words[1][0]}`.toUpperCase();
  return label.trim().slice(0, 2).toUpperCase();
}

/** Narrowest useful sidebar: General agents and the selected workspace's orchestrators only. */
export function SidebarCompactRail({ groups, tabStatuses, onActivate }: Props) {
  const [peek, setPeek] = useState<Peek | null>(null);
  const releaseTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hold = () => {
    if (releaseTimer.current) clearTimeout(releaseTimer.current);
    releaseTimer.current = null;
  };
  const release = () => {
    hold();
    releaseTimer.current = setTimeout(() => setPeek(null), PEEK_RELEASE_MS);
  };
  useEffect(() => hold, []);

  const visible = groups.filter((group) => group.tabs.length > 0);
  const activate = (workspaceId: string, tabId: string) => {
    setPeek(null);
    onActivate(workspaceId, tabId);
  };

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
                onClick={() => activate(group.workspaceId, tab.id)}
                onMouseEnter={(event) => {
                  hold();
                  const rect = event.currentTarget.getBoundingClientRect();
                  setPeek({ tab, workspaceId: group.workspaceId, active, top: rect.top + rect.height / 2, left: rect.right + 10 });
                }}
                onMouseLeave={release}
              >
                {tab.agent_type ? <AgentIcon providerId={tab.agent_type} className="text-[15px]" /> : <TerminalIcon size={14} />}
                <span className="ct-compact-rail__status" aria-hidden="true" />
              </button>
            );
          })}
        </div>
      ))}
      {peek && createPortal(
        <div
          key={peek.tab.id}
          className="ct-compact-peek"
          data-testid="sidebar-compact-peek"
          style={{ top: peek.top, left: peek.left }}
          onMouseEnter={hold}
          onMouseLeave={release}
        >
          <TabItem
            tab={peek.tab}
            index={0}
            isActive={peek.active}
            onClick={() => activate(peek.workspaceId, peek.tab.id)}
            onClose={() => undefined}
            canClose={false}
            workspaceId={peek.workspaceId}
            hasAgent={Boolean(peek.tab.agent_type)}
            status={tabStatuses[peek.tab.id] ?? null}
          />
        </div>,
        document.body,
      )}
    </nav>
  );
}
