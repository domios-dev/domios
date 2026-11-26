import { useEffect, useRef, useState } from "react";
import { Download, X } from "lucide-react";
import { useProviderCliStatus } from "../../hooks/useProviderCliStatus";
import { resolveProviderCliTarget } from "../../lib/providerCliTarget";
import { launchProviderUpdate } from "../../lib/providerUpdate";
import { providerUpdateModel, type ProviderUpdateModel } from "../../lib/providerUpdateModel";
import { Button } from "../shared/Button";

export function ProviderUpdateNotice({ model, onUpdate, onDismiss }: {
  model: ProviderUpdateModel; onUpdate: () => void; onDismiss: () => void;
}) {
  if (!model.message.trim()) return null;
  return <div role="group" aria-label="Provider CLI update" className="ct-ctx-bubble ct-provider-update">
    <Download size={13} aria-hidden="true" className="shrink-0" />
    <span role="status" className="ct-provider-update__message" aria-live="polite">{model.message}</span>
    {model.canUpdate && <Button size="compact" variant="ghost" onClick={onUpdate}>Update</Button>}
    <button type="button" aria-label="Dismiss provider update" onClick={onDismiss} className="shrink-0 rounded p-1 text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]"><X size={12} aria-hidden /></button>
  </div>;
}

export function ProviderUpdateBubble({ providerId, callerTabId, peerId, turnInProgress = false }: {
  providerId: string; callerTabId?: string; peerId?: string | null; turnInProgress?: boolean;
}) {
  const target = resolveProviderCliTarget({ callerTabId, peerId });
  const targetKey = `${target.peerId ?? "local"}:${providerId}`;
  const { status, refresh } = useProviderCliStatus(providerId, target.peerId);
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [launching, setLaunching] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const activeTarget = useRef(targetKey);
  activeTarget.current = targetKey;
  useEffect(() => {
    setLaunching(false);
    setFailure(null);
  }, [targetKey]);
  const model = providerUpdateModel(status?.display_name ?? providerId, status);
  if (turnInProgress || !model || dismissed === `${targetKey}:${model.key}`) return null;
  const update = async () => {
    if (launching) return;
    const launchedFor = targetKey;
    setLaunching(true); setFailure(null);
    try {
      await (target.peerId !== undefined
        ? launchProviderUpdate(providerId, target.callerTabId, target.peerId)
        : launchProviderUpdate(providerId, target.callerTabId));
      if (activeTarget.current === launchedFor) await refresh();
    } catch (reason) {
      if (activeTarget.current === launchedFor) {
        setFailure(reason instanceof Error ? reason.message : String(reason));
      }
    } finally {
      if (activeTarget.current === launchedFor) setLaunching(false);
    }
  };
  const shown = launching ? { ...model, state: "running" as const, message: "Starting CLI update…", canUpdate: false }
    : failure ? { ...model, state: "failed" as const, message: failure } : model;
  return <ProviderUpdateNotice model={shown} onUpdate={() => void update()} onDismiss={() => setDismissed(`${targetKey}:${model.key}`)} />;
}
