import { useEffect, useMemo, useRef, useState } from "react";
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
import { MachineGlyph } from "../shared/MachineGlyph";
import { MachineProvenance } from "../shared/MachineProvenance";
import { usePairedWorkspaceInventory } from "../../store/multiplexer/context";
import { useMeshMachineIdentities } from "../../lib/useMeshMachineIdentities";
import type { GeneralAvailability, ObservedPeerTab } from "../../store/multiplexer/pairedWorkspaceProjection";
import type { MachineIdentity } from "../../lib/machineIdentity";

export interface CompactRailGroup {
  workspaceId: string;
  label: string;
  tabs: Tab[];
  activeTabId: string | null;
  isGeneral?: boolean;
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

interface CompactRailMachine {
  key: string;
  workspaceId: string;
  peerId?: string;
  name: string;
  availability: GeneralAvailability;
  identity?: MachineIdentity;
  sessions: ObservedPeerTab[];
  activeTabId: string | null;
}

interface MachinePeek {
  machineKey: string;
  anchor: HTMLButtonElement;
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
export function SidebarCompactRail({ groups, tabStatuses, onActivate, attentionByTab = {}, touchMode = false }: Props) {
  const [peek, setPeek] = useState<Peek | null>(null);
  const [machinePeek, setMachinePeek] = useState<MachinePeek | null>(null);
  const machinePeekContent = useRef<HTMLDivElement>(null);
  const focusMachineRow = useRef(false);
  const { generals } = usePairedWorkspaceInventory();
  const machineIdentities = useMeshMachineIdentities();
  const releaseTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const hold = () => {
    if (releaseTimer.current) clearTimeout(releaseTimer.current);
    releaseTimer.current = null;
  };
  const release = () => {
    hold();
    releaseTimer.current = setTimeout(() => { setPeek(null); setMachinePeek(null); }, PEEK_RELEASE_MS);
  };
  useEffect(() => hold, []);

  const visible = groups.filter((group) => group.tabs.length > 0);
  const activate = (workspaceId: string, tabId: string) => {
    setPeek(null);
    setMachinePeek(null);
    onActivate(workspaceId, tabId);
  };
  const generalGroup = groups.find(group => group.isGeneral);
  const pairedMachines = useMemo<CompactRailMachine[]>(() => {
    if (!generalGroup || generals.length === 0) return [];
    const localSessions = generalGroup.tabs.map(tab => ({ viewerId: tab.id, tab, status: tabStatuses[tab.id] }));
    const local: CompactRailMachine = {
      key: "local-machine",
      workspaceId: generalGroup.workspaceId,
      name: machineIdentities.local_instance_name || "This machine",
      availability: "online",
      identity: machineIdentities.local_machine_identity,
      sessions: localSessions,
      activeTabId: localSessions.some(session => session.viewerId === generalGroup.activeTabId)
        ? generalGroup.activeTabId
        : null,
    };
    const peers = [...generals].sort((a, b) =>
      a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true }) || a.peerId.localeCompare(b.peerId));
    return [local, ...peers.map(peer => ({
      key: peer.peerId,
      workspaceId: generalGroup.workspaceId,
      peerId: peer.peerId,
      name: peer.name,
      availability: peer.availability,
      identity: machineIdentities.machine_identities?.[peer.peerId],
      sessions: peer.sessions.map((session: ObservedPeerTab) => ({
        ...session,
        tab: session.tab.id === session.viewerId ? session.tab : { ...session.tab, id: session.viewerId },
      })),
      activeTabId: peer.sessions.some(session => session.viewerId === generalGroup.activeTabId)
        ? generalGroup.activeTabId
        : null,
    }))];
  }, [generalGroup, generals, machineIdentities, tabStatuses]);
  const openMachinePeek = (machine: CompactRailMachine, anchor: HTMLButtonElement) => {
    hold();
    const rect = anchor.getBoundingClientRect();
    setPeek(null);
    setMachinePeek({ machineKey: machine.key, anchor, x: rect.right + 10, y: rect.top - 4 });
  };
  const machineGroups = pairedMachines.length > 0 ? pairedMachines : [];
  const activeMachinePeek = machineGroups.find(machine => machine.key === machinePeek?.machineKey);
  useEffect(() => {
    if (!focusMachineRow.current || !activeMachinePeek) return;
    focusMachineRow.current = false;
    machinePeekContent.current?.querySelector<HTMLElement>(".sidebar-tab-activation, .sidebar-paired-general-empty")?.focus();
  }, [activeMachinePeek, machinePeek]);
  const otherVisibleGroups = visible.filter(group => !machineGroups.length || !group.isGeneral);
  const machineStatus = (availability: GeneralAvailability) => availability === "signed-out" ? "signed out"
    : availability === "update-required" ? "update required" : availability;
  const machineActivity = (machine: CompactRailMachine) => {
    if (machine.availability !== "online") return "idle";
    const kinds = machine.sessions.map(session => tabActivity(session.tab, session.status ?? tabStatuses[session.viewerId] ?? null).kind);
    return kinds.includes("working") ? "working"
      : kinds.includes("waiting-on-user") ? "waiting-on-user"
        : kinds.includes("failed") ? "failed" : "idle";
  };

  return (
    <nav className="ct-compact-rail" aria-label="Main agents" data-testid="sidebar-compact-rail">
      {machineGroups.length > 0 && <div className="ct-compact-rail__machine-group" role="group" aria-label="General by machine">
        {machineGroups.map((machine) => {
          const target = machine.activeTabId ?? machine.sessions.find(session => session.tab.protected)?.viewerId ?? machine.sessions[0]?.viewerId;
          const activity = machineActivity(machine);
          const firstRun = machine.key === "local-machine" && machine.sessions.some(session => isFirstRunGeneralTab(session.tab));
          return <button
            key={machine.key}
            type="button"
            className="ct-compact-rail__machine"
            data-active={machine.activeTabId !== null || undefined}
            data-activity={activity}
            data-touch-mode={touchMode || undefined}
            data-first-run-allowed={firstRun || undefined}
            data-first-run-home={firstRun || undefined}
            aria-current={machine.activeTabId ? "page" : undefined}
            aria-label={`Open ${machine.name} General sessions, ${machineStatus(machine.availability)}`}
            aria-expanded={machinePeek?.machineKey === machine.key}
            aria-controls={machinePeek?.machineKey === machine.key ? "sidebar-compact-machine-peek" : undefined}
            title={`${machine.name} · ${machineStatus(machine.availability)}`}
            onClick={(event) => target ? activate(machine.workspaceId, target) : openMachinePeek(machine, event.currentTarget)}
            onMouseEnter={(event) => openMachinePeek(machine, event.currentTarget)}
            onMouseLeave={release}
            onFocus={(event) => openMachinePeek(machine, event.currentTarget)}
            onBlur={release}
            onKeyDown={(event) => {
              if (event.key !== "ArrowDown") return;
              event.preventDefault();
              focusMachineRow.current = true;
              openMachinePeek(machine, event.currentTarget);
            }}
          >
            <MachineGlyph kind={machine.identity?.kind} color={machine.identity?.color} size="rail" label={`${machine.name} machine`} />
            <span className="ct-compact-rail__status" aria-hidden="true" />
            <span className="sr-only">{machine.availability !== "online" ? machineStatus(machine.availability) : activity}</span>
          </button>;
        })}
      </div>}
      {otherVisibleGroups.map((group) => (
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
                data-touch-mode={touchMode || undefined}
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
                {tab.agent_type ? <AgentIcon tab={tab} providerId={tab.agent_type} className="text-[15px]" /> : <TerminalIcon size={14} />}
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
      {machinePeek && activeMachinePeek && (
        <AnchoredPopover open anchor={{ current: machinePeek.anchor }} positionAnchor={{ x: machinePeek.x, y: machinePeek.y }} gap={0} onClose={() => setMachinePeek(null)}>
          <div
            ref={machinePeekContent}
            id="sidebar-compact-machine-peek"
            className="ct-compact-peek ct-compact-machine-peek"
            data-testid="sidebar-compact-machine-peek"
            role="group"
            aria-label={`${activeMachinePeek.name} General sessions`}
            onMouseEnter={hold}
            onMouseLeave={release}
            onFocusCapture={hold}
            onBlurCapture={(event) => {
              if (!event.currentTarget.contains(event.relatedTarget)) release();
            }}
          >
            {activeMachinePeek.sessions.length > 0 ? activeMachinePeek.sessions.map((session, index) => {
              const machine = activeMachinePeek;
              const unavailable = machine.availability !== "online";
              return <TabItem
                key={session.viewerId}
                tab={session.tab}
                index={index}
                isActive={machine.activeTabId === session.viewerId}
                onClick={() => activate(machine.workspaceId, session.viewerId)}
                onClose={() => undefined}
                canClose={false}
                readOnly={machine.key !== "local-machine"}
                workspaceId={machine.workspaceId}
                hasAgent={Boolean(session.tab.agent_type)}
                status={session.status ?? tabStatuses[session.viewerId] ?? null}
                attention={attentionByTab[session.viewerId]}
                touchMode={touchMode}
                machineLine={{
                  peerId: machine.peerId,
                  name: machine.name,
                  status: unavailable ? machineStatus(machine.availability) : undefined,
                  repeatNameForNonProtected: true,
                  suppressActivity: unavailable,
                  suppressAttention: machine.key !== "local-machine",
                }}
              />;
            }) : <div role="status" tabIndex={-1} className="sidebar-paired-general-empty">
              <MachineProvenance
                peerId={activeMachinePeek.peerId}
                name={activeMachinePeek.name}
                status={activeMachinePeek.availability === "online" ? "no General sessions" : machineStatus(activeMachinePeek.availability)}
              />
              <span>· {activeMachinePeek.availability === "online" ? "no General sessions" : machineStatus(activeMachinePeek.availability)}</span>
            </div>}
          </div>
        </AnchoredPopover>
      )}
    </nav>
  );
}
