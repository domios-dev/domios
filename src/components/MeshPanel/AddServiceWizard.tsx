import { useState } from "react";
import { servicesCreate } from "./meshApi";
import type { HealthMode } from "./types";

interface Props {
  onClose: () => void;
  onCreated: () => void;
}

const INPUT =
  "bg-[var(--ct-titlebar-bg)] border border-[var(--ct-border)] rounded px-2 py-1 text-xs w-full text-[var(--ct-fg)]";
const INPUT_MONO = `${INPUT} font-mono`;

export function AddServiceWizard({ onClose, onCreated }: Props) {
  const [name, setName] = useState("");
  const [healthMode, setHealthMode] = useState<HealthMode>("http");
  const [url, setUrl] = useState("");
  const [healthPath, setHealthPath] = useState("/healthz");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [processLabel, setProcessLabel] = useState("");
  const [scriptPath, setScriptPath] = useState("");
  const [pushEnabled, setPushEnabled] = useState(false);
  const [token, setToken] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const canSubmit =
    !!name.trim() &&
    !submitting &&
    (healthMode === "http"
      ? !!url.trim()
      : healthMode === "tcp"
        ? !!host.trim() && !!port.trim()
        : healthMode === "process"
          ? !!processLabel.trim()
          : !!scriptPath.trim());

  const handleSubmit = async () => {
    setSubmitting(true);
    setError(null);
    try {
      const portNum = port ? parseInt(port, 10) : undefined;
      if (healthMode === "tcp" && (portNum == null || isNaN(portNum))) {
        throw new Error("Port must be a number");
      }
      const result = await servicesCreate({
        name: name.trim(),
        kind: "daemon",
        health_mode: healthMode,
        url: healthMode === "http" ? url.trim() : undefined,
        health_path: healthMode === "http" ? healthPath.trim() : undefined,
        host: healthMode === "tcp" ? host.trim() : undefined,
        port: healthMode === "tcp" ? portNum : undefined,
        process_label:
          healthMode === "process" ? processLabel.trim() : undefined,
        script_path: healthMode === "script" ? scriptPath.trim() : undefined,
        push_enabled: pushEnabled,
      });
      if (pushEnabled && result.token) {
        setToken(result.token);
      } else {
        onCreated();
        onClose();
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  };

  if (token) {
    return (
      <div
        className="fixed inset-0 bg-black/60 flex items-center justify-center z-50"
        onClick={onClose}
      >
        <div
          onClick={(e) => e.stopPropagation()}
          className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded-md p-6 w-[480px]"
        >
          <h2 className="text-[var(--ct-fg)] mb-3 text-base font-medium">
            Service registered ✓
          </h2>
          <p className="text-xs text-[var(--ct-yellow)] mb-2">
            Push token (save this NOW — will not be shown again):
          </p>
          <div className="bg-[var(--ct-titlebar-bg)] p-3 rounded font-mono text-xs select-all break-all border border-[var(--ct-yellow)] text-[var(--ct-fg)]">
            {token}
          </div>
          <div className="mt-4 flex justify-end">
            <button
              onClick={() => {
                onCreated();
                onClose();
              }}
              className="px-4 py-2 bg-[var(--ct-accent)] text-[var(--ct-bg)] rounded text-xs font-medium cursor-pointer border-none"
            >
              Done
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      className="fixed inset-0 bg-black/60 flex items-center justify-center z-50"
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded-md p-5 w-[480px] flex flex-col gap-3 max-h-[90vh] overflow-auto"
      >
        <h2 className="text-[var(--ct-fg)] text-base font-medium">
          Add Service
        </h2>

        <Field label="Name">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="my-service"
            className={INPUT}
            autoFocus
          />
        </Field>

        <Field label="Health mode">
          <select
            value={healthMode}
            onChange={(e) => setHealthMode(e.target.value as HealthMode)}
            className={INPUT}
          >
            <option value="http">HTTP</option>
            <option value="tcp">TCP</option>
            <option value="process">Process (launchd / systemd)</option>
            <option value="script">Script</option>
          </select>
        </Field>

        {healthMode === "http" && (
          <>
            <Field label="URL">
              <input
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                placeholder="http://127.0.0.1:7891"
                className={INPUT_MONO}
              />
            </Field>
            <Field label="Health path">
              <input
                value={healthPath}
                onChange={(e) => setHealthPath(e.target.value)}
                placeholder="/healthz"
                className={INPUT_MONO}
              />
            </Field>
          </>
        )}

        {healthMode === "tcp" && (
          <>
            <Field label="Host">
              <input
                value={host}
                onChange={(e) => setHost(e.target.value)}
                placeholder="127.0.0.1"
                className={INPUT_MONO}
              />
            </Field>
            <Field label="Port">
              <input
                value={port}
                onChange={(e) => setPort(e.target.value)}
                placeholder="6379"
                inputMode="numeric"
                className={INPUT_MONO}
              />
            </Field>
          </>
        )}

        {healthMode === "process" && (
          <Field label="Process label (launchd / systemd unit)">
            <input
              value={processLabel}
              onChange={(e) => setProcessLabel(e.target.value)}
              placeholder="com.example.myservice"
              className={INPUT_MONO}
            />
          </Field>
        )}

        {healthMode === "script" && (
          <Field label="Script path">
            <input
              value={scriptPath}
              onChange={(e) => setScriptPath(e.target.value)}
              placeholder="/usr/local/bin/healthcheck.sh"
              className={INPUT_MONO}
            />
          </Field>
        )}

        <label className="flex items-center gap-2 text-xs text-[var(--ct-fg)] cursor-pointer">
          <input
            type="checkbox"
            checked={pushEnabled}
            onChange={(e) => setPushEnabled(e.target.checked)}
          />
          Enable push API (issues a token for the service to push events)
        </label>

        {error && (
          <div className="text-[10px] text-[var(--ct-red)] bg-[var(--ct-bg)] border border-[var(--ct-red)] rounded p-2 break-all">
            {error}
          </div>
        )}

        <div className="flex gap-2 mt-2 justify-end">
          <button
            onClick={onClose}
            className="px-4 py-2 bg-transparent text-[var(--ct-muted)] rounded text-xs cursor-pointer border-none"
          >
            Cancel
          </button>
          <button
            onClick={handleSubmit}
            disabled={!canSubmit}
            className="px-4 py-2 bg-[var(--ct-accent)] text-[var(--ct-bg)] rounded text-xs font-medium cursor-pointer disabled:opacity-50 border-none"
          >
            {submitting ? "Registering…" : "Register"}
          </button>
        </div>
      </div>
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1">
      <label className="text-[10px] text-[var(--ct-muted)] uppercase tracking-wide">
        {label}
      </label>
      {children}
    </div>
  );
}
