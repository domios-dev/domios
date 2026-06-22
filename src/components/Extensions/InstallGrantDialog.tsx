/**
 * Install grant dialog — the capability gate. Before a plugin runs, the user sees
 * exactly what it requests and grants a subset (default-deny). Mirrors design
 * Screen 02. Pure/presentational: the caller supplies the requested permissions
 * and receives the granted subset on confirm.
 */
import { useMemo, useState } from "react";
import { CapabilityChip, capabilityLabel } from "./CapabilityChip";

export interface GrantRequest {
  pluginId: string;
  displayName: string;
  version?: string;
  channel?: string;
  description?: string;
  /** Permission ids the plugin requests (the maximum it can ever use). */
  requested: string[];
  /** Permissions pre-unchecked by default (e.g. not needed). */
  optional?: string[];
}

export function InstallGrantDialog({
  request,
  onCancel,
  onApprove,
}: {
  request: GrantRequest;
  onCancel: () => void;
  onApprove: (granted: string[]) => void;
}) {
  const initial = useMemo(() => {
    const opt = new Set(request.optional ?? []);
    const granted: Record<string, boolean> = {};
    for (const p of request.requested) granted[p] = !opt.has(p);
    return granted;
  }, [request]);
  const [granted, setGranted] = useState<Record<string, boolean>>(initial);

  const grantedCount = Object.values(granted).filter(Boolean).length;
  const toggle = (p: string) => setGranted((g) => ({ ...g, [p]: !g[p] }));

  return (
    <div
      role="dialog"
      aria-modal="true"
      data-testid="extensions-install-grant-dialog"
      data-plugin-id={request.pluginId}
      style={{
        position: "fixed",
        inset: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "rgba(0,0,0,0.5)",
        zIndex: 60,
      }}
    >
      <div
        style={{
          width: 520,
          maxWidth: "92vw",
          background: "var(--ct-bg-elev, #1e1e2e)",
          color: "var(--ct-fg)",
          border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
          borderRadius: 12,
          boxShadow: "0 16px 48px rgba(0,0,0,0.55)",
          padding: "20px 22px",
        }}
      >
        <div style={{ display: "flex", gap: 13, alignItems: "flex-start", marginBottom: 14 }}>
          <div
            style={{
              width: 42,
              height: 42,
              borderRadius: 9,
              background: "rgba(255,255,255,0.04)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              fontSize: 21,
            }}
          >
            🧩
          </div>
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 16, fontWeight: 620 }}>Install {request.displayName}?</div>
            <div
              style={{
                fontFamily: '"JetBrains Mono", ui-monospace, monospace',
                fontSize: 10.5,
                color: "var(--ct-muted)",
                marginTop: 3,
              }}
            >
              {[request.channel && channelLabel(request.channel), request.version && `v${request.version}`].filter(Boolean).join(" · ")}
            </div>
          </div>
        </div>

        {request.description && (
          <p style={{ fontSize: 13, color: "var(--ct-muted)", margin: "0 0 16px", lineHeight: 1.5 }}>
            {request.description}
          </p>
        )}

        <div
          style={{
            fontFamily: '"JetBrains Mono", ui-monospace, monospace',
            fontSize: 9.5,
            letterSpacing: "0.08em",
            textTransform: "uppercase",
            color: "var(--ct-muted)",
            marginBottom: 8,
          }}
        >
          This plugin is requesting
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 2, marginBottom: 18 }}>
          {request.requested.map((p) => {
            const on = granted[p];
            return (
              <label
                key={p}
                data-testid="extensions-install-permission"
                data-permission-id={p}
                style={{ display: "flex", alignItems: "center", gap: 9, padding: "6px 0", cursor: "pointer" }}
              >
                <span
                  onClick={() => toggle(p)}
                  style={{
                    width: 15,
                    height: 15,
                    borderRadius: 4,
                    flex: "none",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    fontSize: 10,
                    background: on ? "var(--ct-accent, #6366f1)" : "transparent",
                    border: on ? "none" : "1px solid var(--ct-border-default, rgba(255,255,255,0.18))",
                    color: "#fff",
                  }}
                >
                  {on ? "✓" : ""}
                </span>
                <span style={{ flex: 1, fontSize: 13, color: on ? "var(--ct-fg)" : "var(--ct-muted)" }}>
                  {capabilityLabel(p)}
                </span>
                <CapabilityChip label={p} variant={on ? "granted" : "denied"} />
              </label>
            );
          })}
        </div>

        <div style={{ display: "flex", gap: 9, justifyContent: "flex-end" }}>
          <button
            data-testid="extensions-install-cancel"
            onClick={onCancel}
            style={{
              fontSize: 12,
              borderRadius: 4,
              padding: "6px 12px",
              border: "1px solid var(--ct-border-default, rgba(255,255,255,0.12))",
              background: "transparent",
              color: "var(--ct-fg)",
              cursor: "pointer",
            }}
          >
            Cancel
          </button>
          <button
            data-testid="extensions-install-approve"
            onClick={() => onApprove(request.requested.filter((p) => granted[p]))}
            disabled={grantedCount === 0}
            style={{
              fontSize: 12,
              borderRadius: 4,
              padding: "6px 12px",
              border: "1px solid var(--ct-accent, #6366f1)",
              background: "var(--ct-accent, #6366f1)",
              color: "#fff",
              fontWeight: 560,
              cursor: grantedCount === 0 ? "not-allowed" : "pointer",
              opacity: grantedCount === 0 ? 0.5 : 1,
            }}
          >
            Install with {grantedCount} permission{grantedCount === 1 ? "" : "s"}
          </button>
        </div>
      </div>
    </div>
  );
}

function channelLabel(channel: string): string {
  if (channel === "codeterm-core") return "Core";
  if (channel === "codeterm-plugins") return "CodeTerm Plugins";
  return channel;
}
