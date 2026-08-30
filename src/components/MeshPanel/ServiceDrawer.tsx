import { invoke } from "../../lib/bridge";
import { useEffect, useState } from "react";
import { serviceIdToCronId, type Service } from "./types";
import type { PeerStatus } from "./useMeshData";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import {
  servicesDelete,
  servicesRegenerateToken,
} from "./meshApi";
import { CronDrawerSection } from "./CronDrawerSection";
import { AddServiceWizard } from "./AddServiceWizard";
import { UI_BUTTON, UI_INPUT, UI_PANEL, UI_TEXT } from "../shared/design";

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
  onActionDone: () => void;
}

const PANEL_BASE = UI_PANEL.drawerBase;
const CLOSE_BUTTON_CLASS =
  `${UI_PANEL.drawerClose} border border-[var(--ct-border)] hover:border-[var(--ct-accent)] hover:text-[var(--ct-accent)]`;

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
  onActionDone,
}: Props) {
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
        onActionDone={onActionDone}
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
      onClose={onClose}
      onActionDone={onActionDone}
    />
  );
}

function PeerDrawer({
  peer,
  isSelf,
  services,
  onClose,
  onActionDone,
}: {
  peer: PeerStatus;
  isSelf: boolean;
  services: Service[];
  onClose: () => void;
  onActionDone: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [showAddWizard, setShowAddWizard] = useState(false);
  const badgeClass = "rounded text-[9px] px-1.5 py-0.5";
  const sectionTextClass = `${UI_TEXT.metaMuted} text-[10px]`;
  const sectionSelectClass = `${UI_INPUT.full} text-[10px] px-2 py-1`;
  const peerActionBtnClass = `${UI_BUTTON.base} ${UI_BUTTON.compact} border`;
  const peerActionDangerClass = `${UI_BUTTON.base} ${UI_BUTTON.compact} border border-[var(--ct-red)]/40 text-[var(--ct-red)]`;
  const sectionListClass = "bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded p-2 flex flex-col gap-1 text-[10px]";

  const handleDisconnect = async () => {
    if (!confirm(`Disconnect from ${peer.name}?`)) return;
    setBusy(true);
    try {
      await invoke("mesh_disconnect", { instanceId: peer.instance_id });
      onActionDone();
      onClose();
    } catch (e) {
      alert(`Disconnect failed: ${e}`);
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
      onActionDone();
    } catch (e) {
      alert(`Set access failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <DrawerBackdrop onClose={onClose} />
      <div className={PANEL_BASE} style={{ width: 360 }}>
        <div className="flex items-center justify-between">
          <strong className="text-[var(--ct-fg)] truncate">{peer.name}</strong>
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

        <Section label={`Services on this peer (${services.length})`}>
          <div className={sectionListClass}>
            {services.length === 0 && (
              <span className="text-[var(--ct-muted)] italic">
                none registered
              </span>
            )}
            {services.map((s) => (
              <div key={`${s.source_peer_id ?? s.source_peer ?? "local"}:${s.id}`} className="flex items-center gap-1.5">
                <span
                  className="w-1.5 h-1.5 rounded-full"
                  style={{
                    background: healthColor(s.health.status),
                  }}
                />
                <span className="text-[var(--ct-fg)]">{s.name}</span>
                <span className="text-[var(--ct-muted)]">· {s.kind}</span>
              </div>
            ))}
          </div>
        </Section>

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

function SvcDrawer({
  service,
  onClose,
  onActionDone,
}: {
  service: Service;
  onClose: () => void;
  onActionDone: () => void;
}) {
  const [showToken, setShowToken] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const sectionTextClass = `${UI_TEXT.metaMuted} text-[10px]`;
  const sectionHealthClass = `${sectionTextClass} text-[var(--ct-fg)] flex flex-col gap-0.5`;
  const serviceActionBtnClass = `${UI_BUTTON.base} ${UI_BUTTON.compact} border`;
  const serviceActionDangerClass = `${UI_BUTTON.base} ${UI_BUTTON.compact} bg-[var(--ct-red)] text-[var(--ct-bg)] border-none font-medium`;
  const pillClass = "rounded px-1.5 py-0.5 text-[9px]";
  const tokenCardClass = "bg-[var(--ct-bg)] border border-[var(--ct-yellow)] rounded p-2 text-[10px]";
  const sourcePeerId = service.source_peer_id;
  const isRemote = !!sourcePeerId;
  const canDelete = service.capabilities?.delete?.allowed ?? !isRemote;
  const actionsDisabled = busy;

  const handleRotate = async () => {
    if (isRemote) return;
    if (!confirm("Rotate push token? Old token will stop working.")) return;
    setBusy(true);
    try {
      const tk = await servicesRegenerateToken(service.id);
      setShowToken(tk);
      onActionDone();
    } catch (e) {
      alert(`Rotate failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const isCron = service.kind === "cron";

  const handleDelete = async () => {
    if (!confirm(`Delete service "${service.name}"?`)) return;
    setBusy(true);
    try {
      if (isRemote && isCron) {
        await invoke("mesh_peer_cron_delete", {
          instanceId: sourcePeerId,
          id: serviceIdToCronId(service.id),
        });
      } else if (isRemote) {
        await invoke("mesh_peer_service_delete", {
          instanceId: sourcePeerId,
          id: service.id,
        });
      } else if (isCron) {
        await invoke("cron_delete", { id: serviceIdToCronId(service.id) });
      } else {
        await servicesDelete(service.id);
      }
      onActionDone();
      onClose();
    } catch (e) {
      alert(`Delete failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <DrawerBackdrop onClose={onClose} />
      <div className={PANEL_BASE} style={{ width: isCron ? "min(860px, calc(100vw - 32px))" : 400 }}>
        <div className="flex items-center justify-between">
          <strong className="text-[var(--ct-fg)] truncate">{service.name}</strong>
          <button
            onClick={onClose}
            className={CLOSE_BUTTON_CLASS}
            title="Close (Esc)"
          >
            ×
          </button>
        </div>

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
          <CronDrawerSection service={service} onUpdate={onActionDone} />
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
          {service.push_enabled && (
            <button
              onClick={handleRotate}
              disabled={actionsDisabled || isRemote}
              className={`${serviceActionBtnClass} flex-1 disabled:opacity-50`}
              title={
                service.stale
                  ? "Cached service state is not actionable"
                  : isRemote
                    ? "Remote token rotation is not wired yet"
                    : undefined
              }
            >
              Rotate push token
            </button>
          )}
          <button
            onClick={handleDelete}
            disabled={actionsDisabled || !canDelete}
            className={`${serviceActionDangerClass} disabled:opacity-50`}
            title={
              service.stale
                ? "Cached service state is not actionable"
                : isRemote && isCron
                  ? "Remote cron delete is not wired yet"
                  : undefined
            }
          >
            Delete service
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
