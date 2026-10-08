import { useState } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { Button } from "../shared/Button";
import { Card } from "../shared/Card";
import { launchProviderInstaller } from "../../lib/providerInstall";
import { missingInstallerPrerequisites, providerCliPresentation } from "../../lib/providerCli";
import { useProviderCliStatus } from "../../hooks/useProviderCliStatus";

export function ProviderCliControls({ providerId, compact = false }: { providerId: string; compact?: boolean }) {
  const { status, error, summary, refresh } = useProviderCliStatus(providerId);
  const [busy, setBusy] = useState(false);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const model = providerCliPresentation(status);
  const install = async () => {
    setBusy(true);
    setLaunchError(null);
    try { await launchProviderInstaller(providerId); }
    catch (error) { setLaunchError(String(error)); }
    finally { setBusy(false); }
  };
  if (model.state === "none") return null;
  const missing = model.state === "missing";
  const dot = model.state === "installed" ? "var(--ct-success, #3fb950)" : missing ? "var(--ct-warning, #d29922)" : "var(--ct-muted)";
  return (
    <div role="group" aria-label={`${providerId} CLI`} className="flex flex-col gap-1.5 text-[12px] text-[var(--ct-muted)]">
      <div className="flex min-w-0 items-center gap-2">
        <span aria-hidden className="h-1.5 w-1.5 flex-none rounded-full" style={{ background: dot }} />
        <span role="status" title={status?.resolved_path ?? undefined} className="min-w-0 truncate">
          {error ? "CLI availability unknown" : missing ? "CLI not installed" : model.label}
        </span>
        {model.canInstall && (
          <Button size="compact" variant="primary" disabled={busy} aria-label={`Install CLI for ${providerId}`} onClick={() => void install()} className="ml-auto">
            {busy ? "Launching…" : "Install"}
          </Button>
        )}
        {!compact && !model.canInstall && missing && (
          <Button size="compact" variant="ghost" onClick={() => void refresh()} className="ml-auto">Re-check</Button>
        )}
      </div>
      {missing && missingInstallerPrerequisites(status).map(item => <p role="status" key={item.binary} className="m-0">Needs {item.binary} first. {item.hint}</p>)}
      {(launchError || error) && <div role="alert" className="text-[var(--ct-danger,#f85149)]">{launchError ?? error}</div>}
      {summary && <p role="status" className="m-0">{summary}</p>}
    </div>
  );
}

export function ProviderInstallCard({ provider }: { provider: ProviderDescriptor }) {
  return <Card className="p-4"><ProviderCliControls providerId={provider.id} /></Card>;
}
