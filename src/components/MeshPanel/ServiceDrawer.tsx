import { invoke } from "../../lib/bridge";
import { useEffect, useRef, useState } from "react";
import type { Service } from "./types";
import type { PeerStatus } from "./useMeshData";
import { inventoryResourceKey } from "./meshInventoryModel";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import { CronDrawerSection } from "./CronDrawerSection";
import { AddServiceWizard } from "./AddServiceWizard";
import { UI_BUTTON, UI_INPUT, UI_PANEL, UI_TEXT } from "../shared/design";
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
  onClose: () => void;
  onOpenResource?: (service: Service, resourceKey: string) => void;
  onActionDone: () => void;
  actionAdapter?: MeshResourceActionAdapter;
  onAnnouncement?: (message: string) => void;
}

const PANEL_BASE = UI_PANEL.drawerBase;
const CLOSE_BUTTON_CLASS =
  `${UI_PANEL.drawerClose} border border-[var(--ct-border)] hover:border-[var(--ct-accent)] hover:text-[var(--ct-accent)]`;
const defaultResourceActionAdapter = createMeshResourceActionAdapter();

function DrawerBackdrop({ onClose }: { onClose: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

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
  onClose,
  onOpenResource,
  onActionDone,
  actionAdapter,
  onAnnouncement,
}: Props) {
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

    return (
      <PeerDrawer
        peer={peer}
        isSelf={isSelf}
        services={services.filter((s) => (isSelf ? !s.source_peer_id && !s.source_peer : s.source_peer === state.id || s.source_peer_id === peer.instance_id))}
        onClose={onClose}
        onOpenResource={onOpenResource}
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
  peer,
  isSelf,
  services,
  onClose,
  onOpenResource,
  onActionDone,
  onAnnouncement,
}: {
  peer: PeerStatus;
  isSelf: boolean;
  services: Service[];
  onClose: () => void;
  onOpenResource?: (service: Service, resourceKey: string) => void;
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
  const peerActionDangerClass = `${UI_BUTTON.base} ${UI_BUTTON.compact} border border-[var(--ct-red)]/40 text-[var(--ct-red)]`;
  const sectionListClass = "bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded p-2 flex flex-col gap-1 text-[10px]";
  const headingRef = useRef<HTMLElement>(null);

  useEffect(() => {
    headingRef.current?.focus({ preventScroll: true });
  }, []);

  const handleDisconnect = async () => {
    if (typeof window !== "undefined" && typeof window.confirm === "function" && !window.confirm(`Disconnect from target machine "${peer.name}"? This cannot be undone.`)) return;
    setBusy(true);
    try {
      await invoke("mesh_disconnect", { instanceId: peer.instance_id });
      const message = `Disconnected from target machine ${peer.name}.`;
      setAnnouncement(message);
      onAnnouncement?.(message);
      onActionDone();
      onClose();
    } catch (e) {
      const message = `Disconnect failed: target machine ${peer.name} rejected or failed the request (${String(e)}).`;
      setAnnouncement(message);
      onAnnouncement?.(message);
    } finally {
      setBusy(false);
    }
  };

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
      <div className={PANEL_BASE} style={{ width: 360 }} role="dialog" aria-modal="true" aria-labelledby="mesh-peer-drawer-heading" data-testid="mesh-peer-drawer">
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
                paired
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

        <PeerResourceSection
          title="Services"
          resources={services.filter((service) => service.kind === "daemon")}
          peer={peer}
          sectionListClass={sectionListClass}
          onOpenResource={onOpenResource}
        />
        <PeerResourceSection
          title="Schedules"
          resources={services.filter((service) => service.kind === "cron")}
          peer={peer}
          sectionListClass={sectionListClass}
          onOpenResource={onOpenResource}
        />

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
            {!isSelf && (
              <button
                onClick={handleDisconnect}
                disabled={busy}
                className={`${peerActionDangerClass} flex-1 disabled:opacity-50`}
              >
                Disconnect
              </button>
            )}
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

function PeerResourceSection({
  title,
  resources,
  peer,
  sectionListClass,
  onOpenResource,
}: {
  title: "Services" | "Schedules";
  resources: Service[];
  peer: PeerStatus;
  sectionListClass: string;
  onOpenResource?: (service: Service, resourceKey: string) => void;
}) {
  const peerState = peer.services_error
    ? `Partial peer fetch failure: ${peer.services_error}`
    : peer.services_stale || !peer.online
      ? peer.online ? "Stale · last known inventory" : "Offline · cached inventory"
      : null;
  return (
    <section className={UI_PANEL.section} role="region" aria-labelledby={`mesh-peer-${title.toLowerCase()}-heading`} data-testid={`mesh-peer-${title.toLowerCase()}-section`}>
      <div id={`mesh-peer-${title.toLowerCase()}-heading`} className={UI_PANEL.sectionLabel}>{title} ({resources.length})</div>
      <div className={sectionListClass}>
        <div className="mb-1 flex items-center justify-between gap-2 text-[9px] text-[var(--ct-muted-fg)]">
          <span>{peerState ?? "Live inventory"}</span>
          {peer.services_error && <span className="text-[var(--ct-yellow)]">partial</span>}
        </div>
        {resources.length === 0 && <span className="text-[var(--ct-muted)] italic">none registered</span>}
        {resources.map((service) => {
          const resourceKey = inventoryResourceKey(service);
          const state = resourceStateText(service);
          return (
            <button
              key={resourceKey}
              type="button"
              className="flex w-full items-center gap-1.5 rounded px-1 py-1 text-left hover:bg-[var(--ct-hover)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--ct-accent)]"
              data-testid="mesh-peer-resource-row"
              data-resource-key={resourceKey}
              aria-label={`Open ${title === "Schedules" ? "schedule" : "service"} ${service.name}, ${state}`}
              onClick={() => onOpenResource?.(service, resourceKey)}
            >
              <span className="h-1.5 w-1.5 shrink-0 rounded-full" style={{ background: healthColor(service.health.status) }} />
              <span className="min-w-0 flex-1 truncate text-[var(--ct-fg)]">{service.name}</span>
              <span className="max-w-[120px] truncate text-[var(--ct-muted)]">{state}</span>
            </button>
          );
        })}
      </div>
    </section>
  );
}

function resourceStateText(service: Service): string {
  const labels: string[] = [];
  if (service.provenance?.freshness === "offline" || service.peer_online === false) labels.push("offline · cached");
  else if (service.provenance?.freshness === "stale" || service.stale) labels.push("stale · last known");
  const unavailable = Object.values(service.capabilities ?? {}).find((capability) => !capability.allowed && capability.unavailable_reason);
  if (unavailable?.unavailable_reason === "not_authorized") labels.push("access denied");
  else if (unavailable?.unavailable_reason) labels.push(unavailable.unavailable_reason.replaceAll("_", " "));
  if (labels.length) return labels.join(" · ");
  return service.kind === "cron" ? service.enabled ? service.last_run_status ?? "idle" : "disabled" : service.health.status;
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
              className={`${pillClass} bg-[var(--ct-titlebar-bg)] text-[var(--ct-muted-fg)]`}
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
