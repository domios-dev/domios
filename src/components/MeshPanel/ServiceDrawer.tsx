import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { serviceIdToCronId, type Service } from "./types";
import type { PeerStatus } from "./useMeshData";
import {
  servicesDelete,
  servicesRegenerateToken,
} from "./meshApi";
import { CronDrawerSection } from "./CronDrawerSection";
import { AddServiceWizard } from "./AddServiceWizard";

type AccessLevel = "full" | "read_only" | "agent_isolated";

interface PeerCtx {
  kind: "peer";
  id: string;
}
interface ServiceCtx {
  kind: "service";
  id: string;
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

const PANEL_BASE =
  "fixed top-9 right-0 bottom-0 bg-[var(--ct-titlebar-bg)] border-l border-[var(--ct-border)] p-4 flex flex-col gap-3 z-50 overflow-auto";

const CLOSE_BUTTON_CLASS =
  "text-[var(--ct-fg)] bg-[var(--ct-bg)] border border-[var(--ct-border)] hover:border-[var(--ct-accent)] hover:text-[var(--ct-accent)] cursor-pointer rounded w-7 h-7 flex items-center justify-center text-lg leading-none";

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
      ? { instance_id: "self", name: thisMachineName, ip: thisMachineIp ?? "", api_port: 0, online: true, access_level: "full" } satisfies PeerStatus
      : null);
    if (!peer) return null;
    return (
      <PeerDrawer
        peer={peer}
        isSelf={isSelf}
        services={services.filter((s) => isSelf ? !s.source_peer : s.source_peer === state.id)}
        onClose={onClose}
        onActionDone={onActionDone}
      />
    );
  }
  const service = services.find((s) => s.id === state.id);
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
          <span className="bg-[var(--ct-accent)] text-[var(--ct-bg)] px-1.5 py-0.5 rounded text-[9px] font-medium">
            this machine
          </span>
        ) : (
          <>
            <span className="bg-[var(--ct-green)] text-[var(--ct-bg)] px-1.5 py-0.5 rounded text-[9px] font-medium">
              paired
            </span>
            <span
              className={`px-1.5 py-0.5 rounded text-[9px] ${peer.online ? "bg-[var(--ct-green)] text-[var(--ct-bg)]" : "bg-[var(--ct-bg)] text-[var(--ct-muted)]"}`}
            >
              {peer.online ? "online" : "offline"}
            </span>
            <span className="bg-[var(--ct-bg)] text-[var(--ct-muted)] px-1.5 py-0.5 rounded text-[9px]">
              {peer.access_level}
            </span>
          </>
        )}
      </div>

      <Section label="IP">
        <div className="font-mono text-[10px] text-[var(--ct-fg)]">
          {peer.ip || "(no address)"}
        </div>
      </Section>

      <Section label="Instance ID">
        <div className="font-mono text-[10px] text-[var(--ct-muted-fg)] break-all">
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
            className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-[10px] text-[var(--ct-fg)] w-full"
          >
            <option value="full">full</option>
            <option value="read_only">read_only</option>
            <option value="agent_isolated">agent_isolated</option>
          </select>
        </Section>
      )}

      <Section label={`Services on this peer (${services.length})`}>
        <div className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded p-2 flex flex-col gap-1 text-[10px]">
          {services.length === 0 && (
            <span className="text-[var(--ct-muted)] italic">
              none registered
            </span>
          )}
          {services.map((s) => (
            <div key={s.id} className="flex items-center gap-1.5">
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
            className="py-1.5 bg-transparent text-[var(--ct-accent)] border border-dashed border-[var(--ct-accent)] rounded text-[10px] cursor-pointer hover:bg-[var(--ct-accent)]/10"
          >
            + Add service
          </button>
        )}
        <div className="flex gap-1.5">
          <button
            onClick={onActionDone}
            disabled={busy}
            className="flex-1 py-1.5 bg-[var(--ct-bg)] text-[var(--ct-fg)] border border-[var(--ct-border)] rounded text-[10px] cursor-pointer disabled:opacity-50"
          >
            Refresh
          </button>
          {!isSelf && (
            <button
              onClick={handleDisconnect}
              disabled={busy}
              className="flex-1 py-1.5 bg-[var(--ct-red)] text-[var(--ct-bg)] border-none rounded text-[10px] cursor-pointer font-medium disabled:opacity-50"
            >
              Disconnect
            </button>
          )}
        </div>
      </div>
      {showAddWizard && (
        <AddServiceWizard
          onClose={() => setShowAddWizard(false)}
          onCreated={() => { onActionDone(); setShowAddWizard(false); }}
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

  const handleProbe = async () => {
    setBusy(true);
    try {
      await invoke("services_probe", { id: service.id });
      onActionDone();
    } catch (e) {
      alert(`Probe not yet wired: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const handleRotate = async () => {
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
      if (isCron) {
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
      <div className={PANEL_BASE} style={{ width: 400 }}>
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
        <span className="bg-[var(--ct-bg)] text-[var(--ct-muted-fg)] px-1.5 py-0.5 rounded text-[9px]">
          {service.kind}
        </span>
        <span
          className="px-1.5 py-0.5 rounded text-[9px] font-medium text-[var(--ct-bg)]"
          style={{ background: healthColor(service.health.status) }}
        >
          {service.health.status}
        </span>
        {service.push_enabled && (
          <span className="bg-[var(--ct-accent)] text-[var(--ct-bg)] px-1.5 py-0.5 rounded text-[9px] font-medium">
            push
          </span>
        )}
        {!service.enabled && (
          <span className="bg-[var(--ct-yellow)] text-[var(--ct-bg)] px-1.5 py-0.5 rounded text-[9px] font-medium">
            disabled
          </span>
        )}
      </div>

      <Section label="Health">
        <div className="text-[10px] text-[var(--ct-fg)] flex flex-col gap-0.5">
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
            className="font-mono text-[10px] text-[var(--ct-blue)] underline break-all hover:opacity-80"
          >
            {service.url}
          </a>
        </Section>
      )}
      {service.host && (
        <Section label="Host / Port">
          <div className="font-mono text-[10px] text-[var(--ct-fg)]">
            {service.host}
            {service.port != null ? `:${service.port}` : ""}
          </div>
        </Section>
      )}
      {service.process_label && (
        <Section label="Process label">
          <div className="font-mono text-[10px] text-[var(--ct-fg)] break-all">
            {service.process_label}
          </div>
        </Section>
      )}
      {service.script_path && (
        <Section label="Script path">
          <div className="font-mono text-[10px] text-[var(--ct-fg)] break-all">
            {service.script_path}
          </div>
        </Section>
      )}

      {isCron && (
        <CronDrawerSection service={service} onUpdate={onActionDone} />
      )}

      {showToken && (
        <div className="bg-[var(--ct-bg)] border border-[var(--ct-yellow)] rounded p-2 text-[10px]">
          <div className="text-[var(--ct-yellow)] font-bold mb-1">
            New token (save NOW — will not be shown again):
          </div>
          <div className="font-mono break-all select-all text-[var(--ct-fg)]">
            {showToken}
          </div>
        </div>
      )}

      <div className="mt-auto flex flex-col gap-1.5">
        {!isCron && (
          <button
            onClick={handleProbe}
            disabled={busy}
            className="py-1.5 bg-[var(--ct-bg)] text-[var(--ct-fg)] border border-[var(--ct-border)] rounded text-[10px] cursor-pointer disabled:opacity-50"
          >
            Probe now
          </button>
        )}
        {service.push_enabled && (
          <button
            onClick={handleRotate}
            disabled={busy}
            className="py-1.5 bg-[var(--ct-bg)] text-[var(--ct-fg)] border border-[var(--ct-border)] rounded text-[10px] cursor-pointer disabled:opacity-50"
          >
            Rotate push token
          </button>
        )}
        <button
          onClick={handleDelete}
          disabled={busy}
          className="py-1.5 bg-[var(--ct-red)] text-[var(--ct-bg)] border-none rounded text-[10px] cursor-pointer font-medium disabled:opacity-50"
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
    <div className="text-[10px] flex flex-col gap-1">
      <div className="text-[var(--ct-muted)] uppercase tracking-wide text-[9px]">
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
