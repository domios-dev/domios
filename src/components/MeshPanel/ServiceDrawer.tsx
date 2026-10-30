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
import { UI_BUTTON, UI_INPUT, UI_PANEL, UI_TEXT } from "../shared/design";
import { useEscapeDismissal } from "../shared/useEscapeDismissal";
import {
  capabilityFor,
  capabilitySummary,
  createMeshResourceActionAdapter,
  mutationFailure,
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
export type DrawerState = PeerCtx | ServiceCtx | null;

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

  if (state.kind === "peer") {
    const isSelf = state.id === thisMachineName;
    const peer = peers.find((p) => p.name === state.id) ?? (isSelf
      ? { instance_id: "self", name: thisMachineName, ip: thisMachineIp ?? "", api_port: 0, online: true, access_level: "full", last_seen: null, relay_protocol_version: RELAY_PROTOCOL_VERSION } satisfies PeerStatus
      : null);
    if (!peer) return null;

    if (selectedResource) return <SvcDrawer service={selectedResource} machineName={thisMachineName} onClose={() => setSelectedResource(null)} onActionDone={onActionDone} actionAdapter={actionAdapter} onAnnouncement={onAnnouncement} />;
    return (
      <PeerDrawer
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
  const [busy, setBusy] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const [showAddWizard, setShowAddWizard] = useState(false);
  const badgeClass = "rounded text-[9px] px-1.5 py-0.5";
  const sectionTextClass = `${UI_TEXT.metaMuted} text-[10px]`;
  const sectionSelectClass = `${UI_INPUT.full} text-[10px] px-2 py-1`;
  const peerActionBtnClass = `${UI_BUTTON.base} ${UI_BUTTON.compact} border`;
  const headingRef = useRef<HTMLElement>(null);

  useEffect(() => {
    headingRef.current?.focus({ preventScroll: true });
  }, []);

  const handleAccessChange = async (
    level: AccessLevel,
  ) => {
    setBusy(true);
    try {
      await invoke("mesh_set_access", {
        instanceId: peer.instance_id,
        accessLevel: level,
      });
      const message = `Access level updated on target machine ${peer.name}.`;
      setAnnouncement(message);
      onAnnouncement?.(message);
      onActionDone();
    } catch (e) {
      const message = `Set access failed: target machine ${peer.name} rejected or failed the request (${String(e)}).`;
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
          <strong ref={headingRef} id="mesh-peer-drawer-heading" tabIndex={-1} className="text-[var(--ct-fg)] truncate focus:outline-none">{peer.name}</strong>
          <button
            onClick={onClose}
            className={CLOSE_BUTTON_CLASS}
            title="Close (Esc)"
          >
            ×
          </button>
        </div>

        <div className="flex gap-1.5 flex-wrap">
          {isSelf ? (
            <span className={`${badgeClass} bg-[var(--ct-accent)] text-[var(--ct-bg)] font-medium`}>
              this machine
            </span>
          ) : (
            <>
              <span className={`${badgeClass} bg-[var(--ct-green)] text-[var(--ct-bg)] font-medium`}>
                {peer.pending ? "pending" : "paired"}
              </span>
              <span
                className={`${badgeClass} ${peer.online ? "bg-[var(--ct-green)] text-[var(--ct-bg)]" : "bg-[var(--ct-bg)] text-[var(--ct-muted)]"}`}
              >
                {peer.online ? "online" : "offline"}
              </span>
              <span className={`${badgeClass} bg-[var(--ct-bg)] text-[var(--ct-muted)]`}>
                {peer.access_level}
              </span>
            </>
          )}
        </div>

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
              value={peer.access_level}
              disabled={busy}
              onChange={(e) =>
                handleAccessChange(e.target.value as AccessLevel)
              }
              className={sectionSelectClass}
            >
              <option value="full">full</option>
              <option value="read_only">read_only</option>
              <option value="agent_isolated">agent_isolated</option>
            </select>
          </Section>
        )}

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
