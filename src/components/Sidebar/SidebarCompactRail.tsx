import { useEffect, useRef, useState, type MouseEvent } from "react";
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
import { RemoteTabIcon, useRemoteTabPresentation, type ObservedTabMachine } from "./RemoteTabPresentation";
import { matchesSidebarMachine, useSidebarMachineFilter } from "./sidebarMachineFilter";

export interface CompactRailGroup {
  workspaceId: string;
  label: string;
  tabs: Tab[];
  activeTabId: string | null;
  observed?: ReadonlyMap<string, { machine: ObservedTabMachine; status?: TabStatus }>;
}

interface Props {
  groups: CompactRailGroup[];
  attentionByTab?: Readonly<Record<string, TabAttention>>;
  tabStatuses: Record<string, TabStatus>;
  onActivate: (workspaceId: string, tabId: string) => void;
  touchMode?: boolean;
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

function RailTab({ tab, observed, status, attention, active, touchMode, onActivate, onPeek, onLeave }: {
  tab: Tab; observed?: ObservedTabMachine; status: TabStatus | null; attention?: TabAttention; active: boolean; touchMode: boolean;
  onActivate: () => void; onPeek: (event: MouseEvent<HTMLButtonElement>) => void; onLeave: () => void;
}) {
  const title = displayTitleForTab(tab);
  const activity = tabActivity(tab, status);
  const machine = useRemoteTabPresentation(tab, activity, observed);
  const firstRun = isFirstRunGeneralTab(tab);
  const icon = tab.agent_type ? <AgentIcon tab={tab} providerId={tab.agent_type} title={machine ? "" : undefined} className="text-[15px]" /> : <TerminalIcon size={14} />;
  const mark = <TabAttentionMark title={title} attention={attention ?? tabAttention(tab, status, [], active)} active={active} placement="compact" remote={Boolean(machine)} />;
  const filter = useSidebarMachineFilter();
  const activityKind = machine?.unavailable ? "idle" : activity.kind;
  return <button
    type="button"
    hidden={!matchesSidebarMachine(filter, machine?.peerId)}
    data-first-run-allowed={firstRun || undefined}
    data-first-run-home={firstRun || undefined}
    aria-disabled={observed?.cached || (firstRun ? false : undefined)}
    className="ct-compact-rail__tab"
    data-active={active || undefined}
    data-activity={activityKind}
    data-touch-mode={touchMode || undefined}
    aria-current={active ? "page" : undefined}
    aria-label={machine ? `Open tab ${title}, ${machine.label}` : `Open tab ${title}`}
    onClick={observed?.cached ? undefined : onActivate}
    onMouseEnter={machine ? undefined : onPeek}
    onMouseLeave={machine ? undefined : onLeave}
  >
    {machine ? <RemoteTabIcon machine={machine}>{icon}{mark}</RemoteTabIcon> : <>{icon}{mark}</>}
    {activityKind === "working" && !attention?.showMark && <span className="ct-compact-rail__status" aria-hidden="true" />}
  </button>;
}

export function SidebarCompactRail({ groups, tabStatuses, onActivate, attentionByTab = {}, touchMode = false }: Props) {
  const [peek, setPeek] = useState<Peek | null>(null);
  const releaseTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const filter = useSidebarMachineFilter();
  const hold = () => {
    if (releaseTimer.current) clearTimeout(releaseTimer.current);
    releaseTimer.current = null;
  };
  const release = () => {
    hold();
    releaseTimer.current = setTimeout(() => setPeek(null), PEEK_RELEASE_MS);
  };
  useEffect(() => hold, []);

  const activate = (workspaceId: string, tabId: string) => {
    setPeek(null);
    onActivate(workspaceId, tabId);
  };

  return (
    <nav className="ct-compact-rail" aria-label="Main agents" data-testid="sidebar-compact-rail">
      {groups.filter(group => group.tabs.length).map(group => (
        <div key={group.workspaceId} className="ct-compact-rail__group" role="group" aria-label={group.label}
          hidden={!group.tabs.some(tab => matchesSidebarMachine(filter, group.observed?.get(tab.id)?.machine.peerId ?? tab.remote_peer_id))}>
          <span className="ct-compact-rail__label" title={group.label} aria-hidden="true">{monogram(group.label)}</span>
          {group.tabs.map(tab => {
            const observed = group.observed?.get(tab.id);
            const active = tab.id === group.activeTabId;
            return <RailTab key={tab.id} tab={tab} active={active} observed={observed?.machine} touchMode={touchMode}
              status={tabStatuses[tab.id] ?? observed?.status ?? null} attention={attentionByTab[tab.id]}
              onActivate={() => activate(group.workspaceId, tab.id)}
              onPeek={event => {
                hold();
                const rect = event.currentTarget.getBoundingClientRect();
                setPeek({ tab, workspaceId: group.workspaceId, active, x: rect.right + 10, y: rect.top - 4 });
              }} onLeave={release} />;
          })}
        </div>
      ))}
      {peek && matchesSidebarMachine(filter) && (
        <AnchoredPopover open anchor={{ x: peek.x, y: peek.y }} gap={0}>
          <div key={peek.tab.id} className="ct-compact-peek" data-testid="sidebar-compact-peek" onMouseEnter={hold} onMouseLeave={release}>
            <TabItem tab={peek.tab} index={0} isActive={peek.active} onClick={() => activate(peek.workspaceId, peek.tab.id)}
              onClose={() => undefined} canClose={false} workspaceId={peek.workspaceId} hasAgent={Boolean(peek.tab.agent_type)}
              status={tabStatuses[peek.tab.id] ?? null} attention={attentionByTab[peek.tab.id]} />
          </div>
        </AnchoredPopover>
      )}
    </nav>
  );
}
