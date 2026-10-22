import { useState } from "react";
import { Cpu } from "lucide-react";
import { Button } from "../../shared/Button";
import { SettingsCard, SettingsPill } from "../shared";
import {
  activeOption,
  embedderOptions,
  isReembedding,
  progressLabel,
  switchSummary,
  type Refusal,
  type SwitchPreview,
} from "./embedderModel";
import { useEmbedder } from "./useEmbedder";
import type { AppConfig } from "../../../lib/types";
import { daemonAuthToken, daemonHttpBase } from "../../../lib/webApi";

export function EmbedderCard({ config }: { config: AppConfig }) {
  const token = daemonAuthToken(config.api);
  const { listing, status, error, preview, switchTo } = useEmbedder(
    daemonHttpBase(config.api),
    token ? { Authorization: `Bearer ${token}` } : {},
  );
  const [choice, setChoice] = useState<string | null>(null);
  const [pending, setPending] = useState<SwitchPreview | null>(null);
  const [refusal, setRefusal] = useState<Refusal | null>(null);
  const [busy, setBusy] = useState(false);

  if (!listing || !status) {
    return (
      <SettingsCard icon={<Cpu size={16} />} title="Memory embedder">
        <p className="text-xs text-[var(--ct-muted)]">{error ?? "Loading…"}</p>
      </SettingsCard>
    );
  }

  const options = embedderOptions(listing);
  const current = activeOption(status);
  const selected = choice ?? current;
  const disabledReasons = options.filter((option) => option.disabled && option.reason);

  const choose = async (value: string) => {
    setChoice(value);
    setPending(null);
    setRefusal(null);
    if (value === current) return;
    try {
      const next = await preview(value);
      if (next.refusal) setRefusal(next.refusal);
      else setPending(next);
    } catch (reason) {
      setRefusal(reason as Refusal);
    }
  };

  const confirm = async () => {
    setBusy(true);
    try {
      await switchTo(selected);
      setPending(null);
      setChoice(null);
    } catch (reason) {
      setRefusal(reason as Refusal);
    } finally {
      setBusy(false);
    }
  };

  return (
    <SettingsCard
      icon={<Cpu size={16} />}
      title="Memory embedder"
      headerRight={<SettingsPill tone={status.available ? "green" : "muted"}>{status.available ? "available" : "unavailable"}</SettingsPill>}
      description="One embedder turns every memory into vectors for semantic search. Switching re-embeds everything in the background; keyword search keeps working throughout."
    >
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-xs" aria-label="Embedder status">
        <dt>Provider</dt>
        <dd data-testid="embedder-provider">{status.provider}{status.remote ? " (remote)" : ""}</dd>
        <dt>Model</dt>
        <dd data-testid="embedder-model">{status.model}</dd>
        <dt>Dimension</dt>
        <dd data-testid="embedder-dimension">{status.dimension}</dd>
        <dt>Re-embed</dt>
        <dd>
          <span data-testid="embedder-progress">{progressLabel(status)}</span>
          {isReembedding(status) && (
            <progress className="ml-2 align-middle" max={status.reembed.total} value={status.reembed.done} aria-label="Re-embed progress" />
          )}
        </dd>
        <dt>Vector pending</dt>
        <dd data-testid="embedder-pending">{status.pending}</dd>
      </dl>

      <label className="ct-settings-toggle">
        <span>Embedder</span>
        <select
          aria-label="Embedder"
          className="ct-field w-72"
          value={selected}
          disabled={busy || isReembedding(status)}
          onChange={(event) => void choose(event.target.value)}
        >
          {options.map((option) => (
            <option key={option.value} value={option.value} disabled={option.disabled}>
              {option.label}
            </option>
          ))}
        </select>
      </label>

      {disabledReasons.length > 0 && (
        <ul className="text-xs text-[var(--ct-muted)]" aria-label="Unavailable embedders">
          {[...new Set(disabledReasons.map((option) => option.reason))].map((reason) => (
            <li key={reason}>{reason}</li>
          ))}
          {listing.providers
            .flatMap((provider) => provider.refusal?.hints ?? [])
            .filter((hint, index, all) => all.indexOf(hint) === index)
            .map((hint) => (
              <li key={hint}>
                <code className="ct-detail__mono">{hint}</code>
              </li>
            ))}
        </ul>
      )}

      {pending && (
        <div role="region" aria-label="Confirm embedder switch" className="flex flex-col gap-2 text-xs">
          <p data-testid="embedder-switch-count">{switchSummary(pending)}</p>
          {pending.warning && <p role="alert">{pending.warning}</p>}
          <div className="flex gap-2">
            <Button onClick={() => void confirm()} disabled={busy}>
              Switch and re-embed
            </Button>
            <Button variant="ghost" onClick={() => { setPending(null); setChoice(null); }} disabled={busy}>
              Cancel
            </Button>
          </div>
        </div>
      )}

      {refusal && (
        <div role="alert" className="text-xs text-[var(--ct-red)]">
          <p>{refusal.message}</p>
          {refusal.hints.map((hint) => (
            <code key={hint} className="ct-detail__mono block">{hint}</code>
          ))}
        </div>
      )}
    </SettingsCard>
  );
}
