import { useState } from "react";
import { CodeBlock } from "@codeterm/chat-ui";
import type { ProviderDescriptor } from "../../lib/types";
import { Button } from "../shared/Button";
import { Card } from "../shared/Card";
import { launchProviderInstaller } from "../../lib/providerInstall";
import { providerCliView, type ProviderCliState } from "../../lib/providerCli";
import { useProviderCliStatus } from "../../hooks/useProviderCliStatus";

const STATE_DOT: Record<ProviderCliState, string> = {
  checking: "var(--ct-muted)",
  none: "var(--ct-muted)",
  installed: "var(--ct-green)",
  missing: "var(--ct-yellow, #d97706)",
  unknown: "var(--ct-muted)",
};

export function ProviderCliControls({ providerId, compact = false }: { providerId: string; compact?: boolean }) {
  const { status, error, summary, refresh } = useProviderCliStatus(providerId);
  const [busy, setBusy] = useState(false);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const view = providerCliView(status, error);
  const install = async () => {
    setBusy(true);
    setLaunchError(null);
    try { await launchProviderInstaller(providerId); }
    catch (error) { setLaunchError(String(error)); }
    finally { setBusy(false); }
  };
  if (view.state === "none") return null;
  const body = (
    <div role="group" aria-label={`${providerId} CLI`} data-cli-state={view.state} className={compact ? "flex flex-col gap-1.5" : "flex flex-col gap-3"}>
      <div className="flex min-h-7 min-w-0 items-center gap-2">
        <span aria-hidden className="h-1.5 w-1.5 flex-none rounded-full" style={{ background: STATE_DOT[view.state] }} />
        <span
          role="status"
          title={view.pathHint ?? (view.state === "unknown" ? error ?? undefined : undefined)}
          className={compact ? "min-w-0 truncate text-[11px] text-[var(--ct-muted-fg)]" : "min-w-0 truncate text-xs font-medium text-[var(--ct-fg)]"}
        >
          {view.label}
          {view.version && <span className="ct-text-mono font-normal text-[var(--ct-muted-fg)]"> · {view.version}</span>}
        </span>
        {(view.canRecheck || view.canInstall) && (
          <div className="ml-auto flex flex-none items-center gap-1.5">
            {view.canRecheck && (
              <Button size="compact" variant="ghost" aria-label={`Re-check CLI for ${providerId}`} onClick={() => void refresh()}>
                Re-check
              </Button>
            )}
            {view.canInstall && (
              <Button size="compact" variant="primary" loading={busy} aria-label={`Install CLI for ${providerId}`} onClick={() => void install()}>
                Install
              </Button>
            )}
          </div>
        )}
      </div>
      {view.compatibilityHint && <p role="status" aria-label="CLI compatibility" className="m-0 text-[11px] text-[var(--ct-muted-fg)]">{view.compatibilityHint}</p>}
      {!compact && view.installCommand && (
        <div aria-label="Install command" role="region">
          <CodeBlock text={view.installCommand} language={view.installLanguage} wrapLines />
        </div>
      )}
      {!compact && view.prerequisites.map(item => (
        <p role="status" key={item.binary} title={item.hint} className="m-0 truncate text-[11px] text-[var(--ct-muted-fg)]">
          Needs {item.binary} first. {item.hint}
        </p>
      ))}
      {launchError && <p role="alert" className="m-0 text-[11px] text-[var(--ct-red)]">{launchError}</p>}
      {summary && <p role="status" className="m-0 text-[11px] text-[var(--ct-muted-fg)]">{summary}</p>}
    </div>
  );
  return compact ? body : <Card className="p-4" data-testid="provider-cli-card">{body}</Card>;
}

export function ProviderInstallCard({ provider }: { provider: ProviderDescriptor }) {
  return <ProviderCliControls providerId={provider.id} />;
}
