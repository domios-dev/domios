import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import type { ProviderDescriptor } from "../../lib/types";
import { Button } from "../shared/Button";
import { Card } from "../shared/Card";
import { Chip } from "../shared/Chip";

interface InstallTabResult {
  tab_id: string;
  tab_id: string;
}

type Status = "checking" | "installed" | "missing";

/// Per-provider Install card on the Agent settings screen. Probes
/// `which <cli_binary>` and renders an Install button that spawns a
/// terminal tab running the provider's install_command. Hidden when
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
      await invoke<InstallTabResult>("install_provider_in_tab", { providerId: provider.id });
    } catch (e) {
      setError(typeof e === "string" ? e : "Install failed");
    } finally {
      setBusy(false);
    }
  }, [provider.id]);

  if (!cliBinary) return null;

  return (
    <Card className="p-4 space-y-3">
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
        <Button
          variant="ghost"
          size="compact"
          onClick={refresh}
          disabled={status === "checking"}
          className="text-[10px]"
        >
          Re-check
        </Button>
        {installCommand && status !== "installed" && (
          <Button
            variant="primary"
            size="compact"
            onClick={onInstall}
            disabled={busy}
            className="text-[11px]"
          >
            {busy ? "Spawning…" : "Install"}
          </Button>
        )}
      </div>

      {error && <div className="text-[10px] text-[var(--ct-red)]">{error}</div>}
    </Card>
  );
}

function StatusBadge({ status }: { status: Status }) {
  if (status === "checking") {
    return <Chip variant="muted" className="text-[9px]">checking…</Chip>;
  }
  if (status === "installed") {
    return <Chip variant="success" className="text-[9px]">installed</Chip>;
  }
  return <Chip variant="danger" className="text-[9px]">not installed</Chip>;
}
