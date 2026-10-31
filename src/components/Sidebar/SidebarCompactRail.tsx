import { useEffect, useRef, useState } from "react";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import type { Tab, TabStatus } from "../../lib/types";
import { tabActivity } from "../../lib/tabActivity";
import { displayTitleForTab } from "../../store/multiplexer/tree";
import { AgentIcon } from "../AgentIcon";
import { TerminalIcon } from "../Icons";
import { tabAttention, type TabAttention } from "./tabAttention";
import { TabAttentionMark } from "./TabAttentionMark";
import { TabItem } from "./TabItem";
import { isFirstRunGeneralTab } from "../AgentPicker/constants";

export interface CompactRailGroup {
  workspaceId: string;
  label: string;
  tabs: Tab[];
  activeTabId: string | null;
}

interface Props {
  groups: CompactRailGroup[];
  attentionByTab?: Readonly<Record<string, TabAttention>>;
  tabStatuses: Record<string, TabStatus>;
  onActivate: (workspaceId: string, tabId: string) => void;
}

interface Peek {
  tab: Tab;
  workspaceId: string;
  active: boolean;
  x: number;
  y: number;
}

const PEEK_RELEASE_MS = 140;

function monogram(label: string): string {
  const words = label.trim().split(/[\s_-]+/).filter(Boolean);
  if (words.length > 1) return `${words[0][0]}${words[1][0]}`.toUpperCase();
  return label.trim().slice(0, 2).toUpperCase();
}

/** Narrowest useful sidebar: General agents and the selected workspace's orchestrators only. */
export function SidebarCompactRail({ groups, tabStatuses, onActivate, attentionByTab = {} }: Props) {
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
                data-first-run-allowed={isFirstRunGeneralTab(tab) || undefined}
                data-first-run-home={isFirstRunGeneralTab(tab) || undefined}
                aria-disabled={isFirstRunGeneralTab(tab) ? false : undefined}
                className="ct-compact-rail__tab"
                data-active={active || undefined}
                data-activity={activity}
                aria-current={active ? "page" : undefined}
                aria-label={`Open tab ${title}`}
                onClick={() => activate(group.workspaceId, tab.id)}
                onMouseEnter={(event) => {
                  hold();
                  const rect = event.currentTarget.getBoundingClientRect();
                  setPeek({ tab, workspaceId: group.workspaceId, active, x: rect.right + 10, y: rect.top - 4 });
                }}
                onMouseLeave={release}
              >
                {tab.agent_type ? <AgentIcon providerId={tab.agent_type} className="text-[15px]" /> : <TerminalIcon size={14} />}
                <TabAttentionMark title={title} attention={attentionByTab[tab.id] ?? tabAttention(tab, tabStatuses[tab.id], [], active)} active={active} placement="compact" />
                {activity === "working" && !attentionByTab[tab.id]?.showMark && <span className="ct-compact-rail__status" aria-hidden="true" />}
              </button>
            );
          })}
        </div>
      ))}
      {peek && (
        <AnchoredPopover open anchor={{ x: peek.x, y: peek.y }} gap={0}>
        <div
          key={peek.tab.id}
          className="ct-compact-peek"
          data-testid="sidebar-compact-peek"
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
            attention={attentionByTab[peek.tab.id]}
          />
        </div>
        </AnchoredPopover>
      )}
    </nav>
  );
}
