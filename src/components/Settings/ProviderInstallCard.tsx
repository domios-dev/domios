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
  return (
    <div role="group" aria-label={`${providerId} CLI`} className="space-y-2">
      <span role="status">{error ? "CLI availability unknown" : model.label}</span>
      {!compact && status?.binary && <div className="text-xs font-mono">{status.resolved_path ?? status.binary}</div>}
      {!compact && status?.installer && <pre className="text-xs overflow-x-auto">{status.installer}</pre>}
      {missingInstallerPrerequisites(status).map(item => <p role="status" key={item.binary}>Missing command prerequisite: {item.binary}. {item.hint}</p>)}
      {model.canInstall && (
        <Button size="compact" variant="primary" disabled={busy} aria-label={`Install CLI for ${providerId}`} onClick={() => void install()}>
          {busy ? "Launching…" : "Install CLI"}
        </Button>
      )}
      {!compact && <Button size="compact" variant="ghost" onClick={() => void refresh()}>Re-check CLI</Button>}
      {!compact && status?.required && !status.installer && status.installed === false && <p>No installer declared for this OS.</p>}
      {(launchError || error) && <div role="alert">{launchError ?? error}</div>}
      {summary && <p role="status">{summary}</p>}
    </div>
  );
}

export function ProviderInstallCard({ provider }: { provider: ProviderDescriptor }) {
  return <Card className="p-4"><ProviderCliControls providerId={provider.id} /></Card>;
}
