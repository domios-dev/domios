import { useMeshPeerName } from "../../lib/useMeshMachineIdentities";
import { LOCAL_MACHINE_ID } from "../../lib/machinePlacement";
import { MachineProvenance } from "../shared/MachineProvenance";
import { ACCESS_LABELS, MachineFooter } from "./MachineFooter";
import { appVersion, buildHash } from "../../lib/buildStamp";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { PeerInventorySections } from "./PeerInventorySections";
import { inventoryResourceKey } from "./meshInventoryModel";
import { peerInventoryModel, type PeerInventory } from "./peerInventoryModel";
import { invoke } from "../../lib/bridge";
import { useEffect, useRef, useState } from "react";
import type { Service } from "./types";
import type { PeerStatus } from "./useMeshData";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import { CronDrawerSection } from "./CronDrawerSection";
import { AddServiceWizard } from "./AddServiceWizard";
import { CloudMachineDrawer } from "./CloudMachineDrawer";
import type { CloudPromoRedemption, CloudReceipt, CloudUsagePayload } from "../../lib/cloudMachines/types";
import { UI_BUTTON, UI_INPUT, UI_PANEL, UI_TEXT } from "../shared/design";
import { useEscapeDismissal } from "../shared/useEscapeDismissal";
import {
  capabilityFor,
  capabilitySummary,
  createMeshResourceActionAdapter,
  mutationFailure,
  isUnconfirmedMutation,
  resourceTarget,
  unavailableFailure,
  type MeshResourceActionAdapter,
} from "./meshResourceActions";

type AccessLevel = "full" | "read_only" | "agent_isolated";

interface PeerCtx {
  kind: "peer";
  id: string;
}
interface ServiceCtx {
  kind: "service";
  id: string;
  sourcePeerId?: string | null;
}
interface CloudCtx {
  kind: "cloud";
  machineId: string;
}
export type DrawerState = PeerCtx | ServiceCtx | CloudCtx | null;

export interface CloudDrawerControls {
  machines: CloudReceipt[];
  usage: CloudUsagePayload | null;
  planId?: string | null;
  idleSleepMinutes: number;
  busyMachineId: string | null;
  actionError: string | null;
  onTopUp: (machine: CloudReceipt, forWake: boolean) => void;
  onRedeem: (code: string) => Promise<CloudPromoRedemption>;
  onSleep: (machineId: string) => Promise<unknown>;
  onArchive?: (machineId: string) => Promise<unknown>;
  onWake: (machineId: string) => Promise<unknown>;
  onDelete: (machineId: string) => Promise<boolean>;
}

interface Props {
  state: DrawerState;
  services: Service[];
  peers: PeerStatus[];
  thisMachineName: string;
  thisMachineIp?: string;
  loading?: boolean;
  error?: string | null;
  onClose: () => void;
  onActionDone: () => void;
  actionAdapter?: MeshResourceActionAdapter;
  onAnnouncement?: (message: string) => void;
  cloudDrawer?: CloudDrawerControls;
}

const PANEL_BASE = UI_PANEL.drawerBase;
const CLOSE_BUTTON_CLASS =
  `${UI_PANEL.drawerClose} border border-[var(--ct-border)] hover:border-[var(--ct-accent)] hover:text-[var(--ct-accent)]`;
const defaultResourceActionAdapter = createMeshResourceActionAdapter();

function DrawerBackdrop({ onClose }: { onClose: () => void }) {
  useEscapeDismissal(onClose);

  return (
    <div
      className="fixed top-9 left-0 right-0 bottom-0 bg-black/30 z-40"
      onClick={onClose}
    />
  );
}

export function ServiceDrawer({
  state,
  services,
  peers,
  thisMachineName,
  thisMachineIp,
  loading = false,
  error,
  onClose,
  onActionDone,
  actionAdapter,
  onAnnouncement,
  cloudDrawer,
}: Props) {
  const [selectedResourceKey, setSelectedResource] = useState<string | null>(null);
  const selectedResource = services.find((service) => inventoryResourceKey(service) === selectedResourceKey);
  useEffect(() => setSelectedResource(null), [state]);
  const originRef = useRef<HTMLElement | null>(null);
  const wasOpenRef = useRef(false);
  if (state && !wasOpenRef.current) {
    const active = document.activeElement;
    originRef.current = active instanceof HTMLElement ? active : null;
    wasOpenRef.current = true;
  } else if (!state && wasOpenRef.current) {
    wasOpenRef.current = false;
  }
  useEffect(() => {
    if (state || !originRef.current) return;
    originRef.current.focus({ preventScroll: true });
    originRef.current = null;
  }, [state]);

  if (!state) return null;

  if (state.kind === "cloud" && cloudDrawer) {
    const machine = cloudDrawer.machines.find((item) => item.machine_id === state.machineId);
    if (!machine) return null;
    const matchedPeer = peers.find(
      (candidate) =>
        candidate.instance_id === machine.peer_instance_id ||
        candidate.instance_id === machine.installation_id ||
        candidate.instance_id === machine.machine_id,
    );
    return (
      <CloudMachineDrawer
        key={machine.machine_id}
        machine={machine}
        machines={cloudDrawer.machines}
        usage={cloudDrawer.usage}
        planId={cloudDrawer.planId}
        idleSleepMinutes={cloudDrawer.idleSleepMinutes}
        reachabilityHint={matchedPeer?.reachabilityHint}
        busy={cloudDrawer.busyMachineId === machine.machine_id}
        actionError={cloudDrawer.actionError}
        onClose={onClose}
        onTopUp={(forWake) => cloudDrawer.onTopUp(machine, forWake)}
        onRedeem={cloudDrawer.onRedeem}
        onSleep={() => cloudDrawer.onSleep(machine.machine_id)}
        onArchive={cloudDrawer.onArchive ? () => cloudDrawer.onArchive!(machine.machine_id) : undefined}
        onWake={() => cloudDrawer.onWake(machine.machine_id)}
        onDelete={() => cloudDrawer.onDelete(machine.machine_id)}
      />
    );
  }

  if (state.kind === "peer") {
    if (cloudDrawer) {
      const cloudMatch = cloudDrawer.machines.find(
        (item) =>
          item.machine_id === state.id ||
          item.peer_instance_id === state.id ||
          item.installation_id === state.id,
      );
      if (cloudMatch) {
        const matchedPeer = peers.find((candidate) => candidate.instance_id === state.id);
        return (
          <CloudMachineDrawer
            key={cloudMatch.machine_id}
            machine={cloudMatch}
            machines={cloudDrawer.machines}
            usage={cloudDrawer.usage}
            planId={cloudDrawer.planId}
            idleSleepMinutes={cloudDrawer.idleSleepMinutes}
            reachabilityHint={matchedPeer?.reachabilityHint}
            busy={cloudDrawer.busyMachineId === cloudMatch.machine_id}
            actionError={cloudDrawer.actionError}
            onClose={onClose}
            onTopUp={(forWake) => cloudDrawer.onTopUp(cloudMatch, forWake)}
            onRedeem={cloudDrawer.onRedeem}
            onSleep={() => cloudDrawer.onSleep(cloudMatch.machine_id)}
            onArchive={cloudDrawer.onArchive ? () => cloudDrawer.onArchive!(cloudMatch.machine_id) : undefined}
            onWake={() => cloudDrawer.onWake(cloudMatch.machine_id)}
            onDelete={() => cloudDrawer.onDelete(cloudMatch.machine_id)}
          />
        );
      }
    }
    const isSelf = state.id === LOCAL_MACHINE_ID;
    const peer = isSelf
      ? { instance_id: "self", name: thisMachineName, ip: thisMachineIp ?? "", api_port: 0, online: true, access_level: "full", last_seen: null, relay_protocol_version: RELAY_PROTOCOL_VERSION } satisfies PeerStatus
      : peers.find((p) => p.instance_id === state.id);
    if (!peer) return null;

    if (selectedResource) return <SvcDrawer service={selectedResource} machineName={thisMachineName} onClose={() => setSelectedResource(null)} onActionDone={onActionDone} actionAdapter={actionAdapter} onAnnouncement={onAnnouncement} />;
    return (
      <PeerDrawer
        key={peer.instance_id}
        peer={peer}
        isSelf={isSelf}
        inventory={peerInventoryModel(services, peer, isSelf, loading, error)}
        onSelect={(service) => setSelectedResource(inventoryResourceKey(service))}
        onClose={onClose}
        onActionDone={onActionDone}
        onAnnouncement={onAnnouncement}
      />
    );
  }

  if (state.kind !== "service") return null;

  const service = services.find(
    (s) =>
      s.id === state.id &&
      (state.sourcePeerId == null
        ? !s.source_peer_id && !s.source_peer
        : (s.source_peer_id ?? s.source_peer) === state.sourcePeerId),
  );
  if (!service) return null;

  return (
    <SvcDrawer
      service={service}
      machineName={thisMachineName}
      onClose={onClose}
      onActionDone={onActionDone}
      actionAdapter={actionAdapter}
      onAnnouncement={onAnnouncement}
    />
  );
}

function PeerDrawer({
  inventory,
  onSelect,
  peer,
  isSelf,
  onClose,
  onActionDone,
  onAnnouncement,
}: {
  inventory: PeerInventory;
  onSelect: (service: Service) => void;
  peer: PeerStatus;
  isSelf: boolean;
  onClose: () => void;
  onActionDone: () => void;
  onAnnouncement?: (message: string) => void;
}) {
  const displayName = useMeshPeerName(isSelf ? undefined : peer.instance_id) ?? peer.name;
  const [busy, setBusy] = useState(false);
  const [accessError, setAccessError] = useState<string | null>(null);
  const [accessLevel, setAccessLevel] = useState(peer.access_level);
  const [announcement, setAnnouncement] = useState("");
  const [showAddWizard, setShowAddWizard] = useState(false);
  const sectionTextClass = `${UI_TEXT.metaMuted} text-[10px]`;
  const sectionSelectClass = `${UI_INPUT.full} text-[10px] px-2 py-1`;
  const peerActionBtnClass = `${UI_BUTTON.base} ${UI_BUTTON.compact} border`;
  const headingRef = useRef<HTMLElement>(null);

  useEffect(() => setAccessLevel(peer.access_level), [peer.access_level]);

  useEffect(() => {
    headingRef.current?.focus({ preventScroll: true });
  }, []);

  const handleAccessChange = async (
    level: AccessLevel,
  ) => {
    setBusy(true);
    setAccessError(null);
    try {
      await invoke("mesh_set_access", {
        instanceId: peer.instance_id,
        accessLevel: level,
      });
      setAccessLevel(level);
      const message = `${displayName} now has ${ACCESS_LABELS[level]} here.`;
      setAnnouncement(message);
      onAnnouncement?.(message);
      onActionDone();
    } catch (e) {
      setAccessError(String(e));
      const message = `Couldn't change ${displayName}'s access. Try again.`;
      setAnnouncement(message);
      onAnnouncement?.(message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <DrawerBackdrop onClose={onClose} />
      <div className={PANEL_BASE} style={{ width: "min(400px, 100vw)" }} role="dialog" aria-modal="true" aria-labelledby="mesh-peer-drawer-heading" data-testid="mesh-peer-drawer">
        <div className="sr-only" role="status" aria-live="polite" aria-atomic="true" data-testid="mesh-peer-announcement">{announcement}</div>
        <div className="flex items-center justify-between">
          <strong ref={headingRef} id="mesh-peer-drawer-heading" tabIndex={-1} className="text-[var(--ct-fg)] truncate focus:outline-none">
            <MachineProvenance peerId={isSelf ? undefined : peer.instance_id} name={displayName} status={peer.online ? "online" : "offline"} />
          </strong>
          <button
            onClick={onClose}
            className={CLOSE_BUTTON_CLASS}
            title="Close (Esc)"
          >
            ×
          </button>
        </div>

        <MachineFooter label={isSelf ? "This machine" : peer.pending ? "Waiting to pair" : peer.online ? "Connected" : "Unreachable"}
          secondary={peer.reachabilityHint ?? (peer.pending ? "pairs when online" : ACCESS_LABELS[accessLevel] ?? accessLevel)} tone={peer.online ? "ok" : "warning"}
          version={isSelf ? appVersion() : peer.domios_version} build={isSelf ? buildHash() : peer.domios_build} showBuild inset={false} />

        <Section label="IP">
          <div className={`${sectionTextClass} text-[var(--ct-fg)]`}>
            {peer.ip || "(no address)"}
          </div>
        </Section>

        <Section label="Instance ID">
          <div className={`${sectionTextClass} text-[var(--ct-muted-fg)] break-all`}>
            {peer.instance_id}
          </div>
        </Section>

        {!isSelf && (
          <Section label="Access level">
            <select
              aria-label={`Access for ${displayName} here`}
              value={accessLevel}
              disabled={busy}
              onChange={(e) =>
                handleAccessChange(e.target.value as AccessLevel)
              }
              className={sectionSelectClass}
            >
              <option value="full">Full access</option>
              <option value="read_only">Read only</option>
              <option value="agent_isolated">Its own agents only</option>
            </select>
          </Section>
        )}

        {accessError ? <InlineErrorNotice description={{ title: `Couldn't change ${displayName}'s access. Try again.`, cause: "", raw: accessError, actions: [] }} /> : announcement && <p className={sectionTextClass}>{announcement}</p>}

        <PeerInventorySections inventory={inventory} onSelect={onSelect} />

        <div className="mt-auto flex flex-col gap-1.5">
          {isSelf && (
            <button
              onClick={() => setShowAddWizard(true)}
              className={`${peerActionBtnClass} text-[var(--ct-accent)] border-dashed border-[var(--ct-accent)] hover:bg-[var(--ct-accent)]/10`}
            >
              + Add service
            </button>
          )}
          <div className="flex gap-1.5">
            <button
              onClick={onActionDone}
              disabled={busy}
              className={`${peerActionBtnClass} flex-1 disabled:opacity-50`}
            >
              Refresh
            </button>

          </div>
        </div>

        {showAddWizard && (
          <AddServiceWizard
            onClose={() => setShowAddWizard(false)}
            onCreated={() => {
              onActionDone();
              setShowAddWizard(false);
            }}
          />
        )}
      </div>
    </>
  );
}

function SvcDrawer({
  service,
  machineName,
  onClose,
  onActionDone,
  actionAdapter = defaultResourceActionAdapter,
  onAnnouncement,
}: {
  service: Service;
  machineName: string;
  onClose: () => void;
  onActionDone: () => void;
  actionAdapter?: MeshResourceActionAdapter;
  onAnnouncement?: (message: string) => void;
}) {
  const [showToken, setShowToken] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const sectionTextClass = `${UI_TEXT.metaMuted} text-[10px]`;
  const sectionHealthClass = `${sectionTextClass} text-[var(--ct-fg)] flex flex-col gap-0.5`;
  const serviceActionBtnClass = `${UI_BUTTON.base} ${UI_BUTTON.compact} border`;
  const serviceActionDangerClass = `${UI_BUTTON.base} ${UI_BUTTON.compact} bg-[var(--ct-red)] text-[var(--ct-bg)] border-none font-medium`;
  const pillClass = "rounded px-1.5 py-0.5 text-[9px]";
  const tokenCardClass = "bg-[var(--ct-bg)] border border-[var(--ct-yellow)] rounded p-2 text-[10px]";
  const headingRef = useRef<HTMLElement>(null);
  const target = resourceTarget(service, machineName);
  const deleteCapability = capabilityFor(service, "delete");
  const isRemote = target.remote;
  const actionsDisabled = busy;

  const handleRotate = async () => {
    if (target.remote) {
      const message = unavailableFailure("Rotate push token", "unsupported_resource_kind");
      setAnnouncement(message);
      onAnnouncement?.(message);
      return;
    }
    if (typeof window !== "undefined" && typeof window.confirm === "function" && !window.confirm(`Rotate push token for service "${service.name}" on target machine "${target.name}"? The old token will stop working.`)) return;
    setBusy(true);
    try {
      const tk = await actionAdapter.regenerateToken(service);
      setShowToken(tk);
      const message = `Push token rotated successfully on target machine ${target.name}.`;
      setAnnouncement(message);
      onAnnouncement?.(message);
      onActionDone();
    } catch (e) {
      const message = mutationFailure("Rotate push token", target, e);
      setAnnouncement(message);
      onAnnouncement?.(message);
    } finally {
      setBusy(false);
    }
  };

  const isCron = service.kind === "cron";

  useEffect(() => {
    headingRef.current?.focus({ preventScroll: true });
  }, []);

  const handleDelete = async () => {
    if (!deleteCapability.allowed) {
      const message = unavailableFailure(service.kind === "cron" ? "Remove schedule" : "Remove registry entry", deleteCapability.unavailable_reason);
      setAnnouncement(message);
      onAnnouncement?.(message);
      return;
    }
    const label = service.kind === "cron" ? "Remove schedule" : "Remove registry entry";
    if (typeof window !== "undefined" && typeof window.confirm === "function" && !window.confirm(`${label} "${service.name}" from target machine "${target.name}"? This cannot be undone.`)) return;
    setBusy(true);
    try {
      await actionAdapter.remove(service);
      const message = `${label} succeeded on target machine ${target.name}.`;
      setAnnouncement(message);
      onAnnouncement?.(message);
      onActionDone();
      onClose();
    } catch (e) {
      const message = mutationFailure(label, target, e);
      setAnnouncement(message);
      onAnnouncement?.(message);
      if (isUnconfirmedMutation(e)) onActionDone();
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <DrawerBackdrop onClose={onClose} />
      <div className={PANEL_BASE} style={{ width: isCron ? "min(860px, calc(100vw - 32px))" : 400 }} role="dialog" aria-modal="true" aria-busy={busy} aria-labelledby="mesh-service-drawer-heading" data-testid="mesh-service-drawer">
        <div className="flex items-center justify-between">
          <strong ref={headingRef} id="mesh-service-drawer-heading" tabIndex={-1} className="text-[var(--ct-fg)] truncate focus:outline-none">{service.name}</strong>
          <button
            onClick={onClose}
            className={CLOSE_BUTTON_CLASS}
            title="Close (Esc)"
          >
            ×
          </button>
        </div>
        <div className="sr-only" role="status" aria-live="polite" aria-atomic="true" data-testid="mesh-resource-announcement">{announcement}</div>

        <div className="flex gap-1.5 flex-wrap">
          <span className={`${pillClass} bg-[var(--ct-bg)] text-[var(--ct-muted-fg)]`}>
            {service.kind}
          </span>
          <span
            className={`${pillClass} font-medium text-[var(--ct-bg)]`}
            style={{ background: healthColor(service.health.status) }}
          >
            {service.health.status}
          </span>
          {service.push_enabled && (
            <span className={`${pillClass} bg-[var(--ct-accent)] text-[var(--ct-bg)] font-medium`}>
              push
            </span>
          )}
          {isRemote && (
            <span
              className={`${pillClass} bg-[color-mix(in_srgb,var(--ct-fg)_6%,transparent)] text-[var(--ct-muted-fg)]`}
              title={service.source_peer_id ?? undefined}
            >
              {service.source_peer ?? "remote"}
            </span>
          )}
          {service.stale && (
            <span className={`${pillClass} bg-[var(--ct-yellow)] text-[var(--ct-bg)] font-medium`}>
              cached
            </span>
          )}
          {!service.enabled && (
            <span className={`${pillClass} bg-[var(--ct-yellow)] text-[var(--ct-bg)] font-medium`}>
              disabled
            </span>
          )}
        </div>
        <div className="text-[10px] text-[var(--ct-muted-fg)]" data-testid="mesh-resource-capability-summary">
          Target machine: <span className="font-medium text-[var(--ct-fg)]">{target.name}</span> · {capabilitySummary(service)}
        </div>
        {!deleteCapability.allowed && <div className="text-[10px] text-[var(--ct-yellow)]" data-testid="mesh-operation-unavailable-delete">Unavailable: {deleteCapability.unavailable_reason ?? "unsupported_resource_kind"}</div>}

        <Section label="Health">
          <div className={sectionHealthClass}>
            <div>
              mode: <span className="font-mono">{service.health_mode}</span>
              {service.health.latency_ms != null && (
                <>
                  {" · "}
                  latency:{" "}
                  <span className="font-mono">{service.health.latency_ms}ms</span>
                </>
              )}
            </div>
            {service.health.version && (
              <div>
                version:{" "}
                <span className="font-mono">{service.health.version}</span>
              </div>
            )}
            {service.health.uptime_secs != null && (
              <div>
                uptime:{" "}
                <span className="font-mono">{service.health.uptime_secs}s</span>
              </div>
            )}
            {service.health.probed_at != null && (
              <div className="text-[var(--ct-muted)]">
                probed: {fmtTime(service.health.probed_at)}
              </div>
            )}
          </div>
        </Section>

        {service.url && (
          <Section label="URL">
            <a
              href={service.url}
              target="_blank"
              rel="noopener noreferrer"
              className={`${sectionTextClass} text-[var(--ct-blue)] underline break-all hover:opacity-80`}
            >
              {service.url}
            </a>
          </Section>
        )}
        {service.host && (
          <Section label="Host / Port">
            <div className={`${sectionTextClass} text-[var(--ct-fg)]`}>
              {service.host}
              {service.port != null ? `:${service.port}` : ""}
            </div>
          </Section>
        )}
        {service.process_label && (
          <Section label="Process label">
            <div className={`${sectionTextClass} text-[var(--ct-fg)] break-all`}>
              {service.process_label}
            </div>
          </Section>
        )}
        {service.script_path && (
          <Section label="Script path">
            <div className={`${sectionTextClass} text-[var(--ct-fg)] break-all`}>
              {service.script_path}
            </div>
          </Section>
        )}

        {isCron && (
          <CronDrawerSection service={service} machineName={machineName} onUpdate={onActionDone} actionAdapter={actionAdapter} onAnnouncement={onAnnouncement} />
        )}

        {showToken && (
          <div className={tokenCardClass}>
            <div className="text-[var(--ct-yellow)] font-bold mb-1">
              New token (save NOW — will not be shown again):
            </div>
            <div className="font-mono break-all select-all text-[var(--ct-fg)]">
              {showToken}
            </div>
          </div>
        )}

        <div className="mt-auto flex flex-col gap-1.5">
          {service.push_enabled && !target.remote && (
            <button
              onClick={handleRotate}
              disabled={actionsDisabled}
              className={`${serviceActionBtnClass} flex-1 disabled:opacity-50`}
              title="Rotate push token"
            >
              Rotate push token
            </button>
          )}
          <button
            onClick={handleDelete}
            disabled={actionsDisabled || !deleteCapability.allowed}
            className={`${serviceActionDangerClass} disabled:opacity-50`}
            title={deleteCapability.unavailable_reason ?? `Remove ${isCron ? "schedule" : "registry entry"}`}
          >
            {isCron ? "Remove schedule" : "Remove registry entry"}
          </button>
        </div>
      </div>
    </>
  );
}

function Section({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className={UI_PANEL.section}>
      <div className={UI_PANEL.sectionLabel}>
        {label}
      </div>
      {children}
    </div>
  );
}

function healthColor(status: string): string {
  switch (status) {
    case "healthy":
      return "var(--ct-green)";
    case "degraded":
      return "var(--ct-yellow)";
    case "down":
      return "var(--ct-red)";
    default:
      return "var(--ct-muted)";
  }
}

function fmtTime(epoch: number): string {
  const d = new Date(epoch * 1000);
  return d.toLocaleString();
}
