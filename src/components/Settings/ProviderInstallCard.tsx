import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ProviderDescriptor } from "../../lib/types";

interface InstallPaneResult {
  tab_id: string;
  pane_id: string;
}

type Status = "checking" | "installed" | "missing";

/// Per-provider Install card on the Agent settings screen. Probes
/// `which <cli_binary>` and renders an Install button that spawns a
/// terminal pane running the provider's install_command. Hidden when
/// the provider declares no CLI binary (nothing to install).
export function ProviderInstallCard({ provider }: { provider: ProviderDescriptor }) {
  const cliBinary = provider.cli_binary ?? null;
  const installCommand = provider.install_command ?? null;
  const [status, setStatus] = useState<Status>("checking");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!cliBinary) return;
    setStatus("checking");
    try {
      const installed = await invoke<boolean>("check_cli_installed", { binary: cliBinary });
      setStatus(installed ? "installed" : "missing");
    } catch {
      setStatus("missing");
    }
  }, [cliBinary]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const onInstall = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke<InstallPaneResult>("install_provider_in_pane", { providerId: provider.id });
    } catch (e) {
      setError(typeof e === "string" ? e : "Install failed");
    } finally {
      setBusy(false);
    }
  }, [provider.id]);

  if (!cliBinary) return null;

  return (
    <div className="rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="text-xs font-medium text-[var(--ct-fg)]">CLI installation</div>
          <div className="text-[10px] text-[var(--ct-muted-fg)] mt-0.5 font-mono truncate">{cliBinary}</div>
        </div>
        <StatusBadge status={status} />
      </div>

      {installCommand ? (
        <div className="text-[10px] text-[var(--ct-muted)] font-mono bg-[var(--ct-bg)] rounded px-2 py-1.5 overflow-x-auto">
          {installCommand}
        </div>
      ) : (
        <div className="text-[10px] text-[var(--ct-muted-fg)]">
          No install command available for this platform — install manually.
        </div>
      )}

      <div className="flex items-center justify-between gap-2">
        <button
          onClick={refresh}
          disabled={status === "checking"}
          className="px-2.5 py-1 text-[10px] font-medium rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:bg-[var(--ct-hover)] transition-colors disabled:opacity-50"
        >
          Re-check
        </button>
        {installCommand && status !== "installed" && (
          <button
            onClick={onInstall}
            disabled={busy}
            className="px-3 py-1 text-[11px] font-medium rounded bg-[var(--ct-accent)] text-white hover:opacity-90 transition-opacity disabled:opacity-50"
          >
            {busy ? "Spawning…" : "Install"}
          </button>
        )}
      </div>

      {error && <div className="text-[10px] text-[var(--ct-red)]">{error}</div>}
    </div>
  );
}

function StatusBadge({ status }: { status: Status }) {
  if (status === "checking") {
    return (
      <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-border)]/40 text-[var(--ct-muted)]">
        checking…
      </span>
    );
  }
  if (status === "installed") {
    return (
      <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-green)]/20 text-[var(--ct-green)]">
        installed
      </span>
    );
  }
  return (
    <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-red)]/20 text-[var(--ct-red)]">
      not installed
    </span>
  );
}
