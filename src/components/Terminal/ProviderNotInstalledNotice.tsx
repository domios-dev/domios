import { useEffect, useRef, useState } from "react";
import { CodeBlock } from "@codeterm/chat-ui/CodeBlock";
import type { ProviderNotInstalledInfo } from "../../lib/providerNotInstalled";
import { installerShellLanguage, missingInstallerPrerequisites } from "../../lib/providerCli";
import { resolveProviderCliTarget } from "../../lib/providerCliTarget";
import { installAndWait } from "../../lib/firstRunInstall";
import { useProviderCliStatus } from "../../hooks/useProviderCliStatus";
import { Button } from "../shared/Button";
import { BrandState } from "../brand/BrandState";
import { brandStateModel } from "../brand/brandStateModel";

export function ProviderNotInstalledNotice({
  tabId,
  info,
  rawReason,
  onRetry,
  onClose,
  inline = false,
}: {
  tabId?: string;
  info: ProviderNotInstalledInfo;
  rawReason: string;
  onRetry?: () => void | Promise<void>;
  onClose?: () => void;
  inline?: boolean;
}) {
  const { peerId } = resolveProviderCliTarget({ callerTabId: tabId });
  const { status, refresh, error: statusError } = useProviderCliStatus(info.providerId, peerId);
  const [installing, setInstalling] = useState(false);
  const [manualCheck, setManualCheck] = useState<"dispatched" | "missing" | "unknown" | "installed" | null>(null);
  const [retrying, setRetrying] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const controllerRef = useRef<AbortController | null>(null);
  useEffect(() => () => {
    controllerRef.current?.abort();
    controllerRef.current = null;
  }, []);

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
      const result = await installAndWait(info.providerId, controller.signal, peerId);
      if (controllerRef.current !== controller) return;
      if (result === "dispatched") {
        setManualCheck("dispatched");
        return;
      }
      setManualCheck(null);
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
      const checked = await refresh();
      if (manualCheck) {
        if (checked?.installed !== true) {
          setManualCheck(checked?.installed === false ? "missing" : "unknown");
          return;
        }
        setManualCheck("installed");
      }
      await onRetry?.();
    } finally {
      setRetrying(false);
    }
  };

  const manualMessage = manualCheck && {
    dispatched: "The installer opened on the remote machine. Retry when it finishes.",
    missing: "The remote machine still reports the CLI is missing.",
    unknown: statusError ?? "The remote machine could not confirm whether the CLI is installed.",
    installed: "The remote machine confirmed the CLI is installed.",
  }[manualCheck];
  const title = manualCheck ? `${info.displayName} installation` : `${info.displayName} isn't installed`;
  const sentence = installError
    ? `${info.displayName} could not be installed automatically. Run the command below manually or retry.`
    : manualMessage ?? `${info.displayName} (${info.binary}) is not installed on this machine. Install it, then retry.`;

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
        {installCommand && manualCheck !== "installed" && manualCheck !== "dispatched" && (
          <Button
            variant="primary"
            loading={installing}
            disabled={busy}
            data-testid="provider-install-action"
            onClick={() => { void handleInstall(); }}
          >
            {installError || manualCheck ? "Retry install" : `Install ${info.displayName}`}
          </Button>
        )}
        {(onRetry || (manualCheck && manualCheck !== "installed")) && (
          <Button
            variant={installCommand ? "ghost" : "primary"}
            loading={retrying}
            disabled={busy}
            data-testid="provider-retry-action"
            onClick={() => { void handleRetry(); }}
          >
            {manualCheck ? "Retry once installed" : "Retry"}
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
        <div className="text-sm font-medium text-[var(--ct-fg)]">{title}</div>
        <p className="m-0 text-xs text-[var(--ct-muted-fg)]">{sentence}</p>
        {body}
      </section>
    );
  }

  return (
    <BrandState model={brandStateModel({ error: { title, sentence } })} className="h-full">
      {body}
    </BrandState>
  );
}
