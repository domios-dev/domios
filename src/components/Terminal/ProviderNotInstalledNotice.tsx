import { useEffect, useRef, useState } from "react";
import { CodeBlock } from "@codeterm/chat-ui/CodeBlock";
import type { ProviderNotInstalledInfo } from "../../lib/providerNotInstalled";
import { installerShellLanguage, missingInstallerPrerequisites } from "../../lib/providerCli";
import { installAndWait } from "../../lib/firstRunInstall";
import { useProviderCliStatus } from "../../hooks/useProviderCliStatus";
import { Button } from "../shared/Button";
import { BrandState } from "../brand/BrandState";
import { brandStateModel } from "../brand/brandStateModel";

export function ProviderNotInstalledNotice({
  info,
  rawReason,
  onRetry,
  onClose,
  inline = false,
}: {
  info: ProviderNotInstalledInfo;
  rawReason: string;
  onRetry?: () => void | Promise<void>;
  onClose?: () => void;
  inline?: boolean;
}) {
  const { status, refresh } = useProviderCliStatus(info.providerId);
  const [installing, setInstalling] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const controllerRef = useRef<AbortController | null>(null);
  useEffect(() => () => { controllerRef.current?.abort(); }, []);

  const installCommand = status?.installer ?? info.installCommand;
  const prerequisites = missingInstallerPrerequisites(status);
  const busy = installing || retrying;

  const handleInstall = async () => {
    if (busy || !installCommand) return;
    const controller = new AbortController();
    controllerRef.current = controller;
    setInstalling(true);
    setInstallError(null);
    try {
      await installAndWait(info.providerId, controller.signal);
      if (controllerRef.current !== controller) return;
      await refresh();
      await onRetry?.();
    } catch (error) {
      if (controllerRef.current !== controller) return;
      setInstallError(error instanceof Error ? error.message : String(error));
    } finally {
      if (controllerRef.current === controller) {
        controllerRef.current = null;
        setInstalling(false);
      }
    }
  };

  const handleRetry = async () => {
    if (busy) return;
    setRetrying(true);
    try {
      await refresh();
      await onRetry?.();
    } finally {
      setRetrying(false);
    }
  };

  const sentence = installError
    ? `${info.displayName} could not be installed automatically. Run the command below manually or retry.`
    : `${info.displayName} (${info.binary}) is not installed on this machine. Install it, then retry.`;

  const body = (
    <div
      data-testid="provider-not-installed-notice"
      className="flex w-full min-w-0 max-w-lg flex-col gap-3 text-left"
    >
      {installing && (
        <p role="status" aria-live="polite" className="m-0 text-xs text-[var(--ct-muted-fg)]">
          Installing {info.displayName}…
        </p>
      )}
      {installError && (
        <div
          role="alert"
          data-testid="provider-install-error"
          className="rounded-md border border-[var(--ct-danger)]/40 bg-[var(--ct-surface)] px-3 py-2 text-xs text-[var(--ct-fg)]"
        >
          <pre className="m-0 max-h-40 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px]">{installError}</pre>
        </div>
      )}
      {installCommand && (
        <div aria-label="Install command" role="region" className="w-full min-w-0">
          <CodeBlock text={installCommand} language={installerShellLanguage(installCommand)} wrapLines />
        </div>
      )}
      {prerequisites.map((item) => (
        <p key={item.binary} role="status" className="m-0 text-xs text-[var(--ct-muted-fg)]">
          Needs {item.binary} first. {item.hint}
        </p>
      ))}
      <div className="flex flex-wrap items-center justify-center gap-2">
        {installCommand && (
          <Button
            variant="primary"
            loading={installing}
            disabled={busy}
            data-testid="provider-install-action"
            onClick={() => { void handleInstall(); }}
          >
            {installError ? "Retry install" : `Install ${info.displayName}`}
          </Button>
        )}
        {onRetry && (
          <Button
            variant={installCommand ? "ghost" : "primary"}
            loading={retrying}
            disabled={busy}
            data-testid="provider-retry-action"
            onClick={() => { void handleRetry(); }}
          >
            Retry
          </Button>
        )}
        {onClose && (
          <Button variant="ghost" disabled={busy} onClick={onClose}>
            Close tab
          </Button>
        )}
      </div>
      {rawReason && (
        <details className="w-full min-w-0 text-xs text-[var(--ct-muted-fg)]">
          <summary className="cursor-pointer">Details</summary>
          <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px]">{rawReason}</pre>
        </details>
      )}
    </div>
  );

  if (inline) {
    return (
      <section
        role="alert"
        className="mx-auto my-2 flex w-full max-w-2xl flex-col gap-3 rounded-lg border border-[var(--ct-border)] bg-[var(--ct-surface)] p-4"
      >
        <div className="text-sm font-medium text-[var(--ct-fg)]">{info.displayName} isn't installed</div>
        <p className="m-0 text-xs text-[var(--ct-muted-fg)]">{sentence}</p>
        {body}
      </section>
    );
  }

  return (
    <BrandState model={brandStateModel({ error: { title: `${info.displayName} isn't installed`, sentence } })} className="h-full">
      {body}
    </BrandState>
  );
}
