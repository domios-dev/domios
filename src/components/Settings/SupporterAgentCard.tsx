import { useEffect, useRef, useState } from "react";
import { ExternalLink, Sparkles } from "lucide-react";
import type { HelpSectionId } from "../../help/types";
import { Button } from "../shared/Button";
import { FormRow } from "../shared/FormRow";
import { AboutLink, InlineLinkButton, SettingsCard, SettingsPill, Toggle } from "./shared";
import { type SupporterAdapter, useDecisionToken } from "./useDecisionToken";

const DOCS_URL = "https://docs.typesafe.ai";

interface Props {
  apiBase: string;
  apiHeaders: HeadersInit;
  onOpenGuide: (section: HelpSectionId) => void;
}

interface ModelCatalog {
  adapters: SupporterAdapter[];
}

function stateLabel(state: string | undefined, loading: boolean, failed: boolean): string {
  if (loading) return "Checking status";
  if (failed) return "Status unavailable";
  switch (state) {
    case "active": return "Active";
    case "token_only": return "Token set, all features off";
    case "token_unusable": return "Token file needs attention";
    case "feature_without_token": return "Feature on, no token";
    case "adapter_unavailable": return "Adapter unavailable";
    case "no_token": return "No token";
    default: return "Status unavailable";
  }
}

function stateTone(state: string | undefined): "green" | "amber" | "accent" | "muted" {
  if (state === "active") return "green";
  if (state === "token_only") return "accent";
  if (state === "no_token") return "muted";
  if (state === "feature_without_token" || state === "token_unusable" || state === "adapter_unavailable") return "amber";
  return "muted";
}

function featureState(value: boolean | undefined): string {
  return value ? "On" : "Off";
}

export function SupporterAgentCard({ apiBase, apiHeaders, onOpenGuide }: Props) {
  const token = useDecisionToken(apiBase, apiHeaders);
  const [catalog, setCatalog] = useState<ModelCatalog | null>(null);
  const [catalogFailed, setCatalogFailed] = useState(false);
  const headersRef = useRef(apiHeaders);
  headersRef.current = apiHeaders;

  useEffect(() => {
    let current = true;
    fetch(`${apiBase}/api/supporter/models`, { headers: headersRef.current })
      .then(async (response) => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return await response.json() as ModelCatalog;
      })
      .then((next) => {
        if (current) {
          setCatalog(next);
          setCatalogFailed(false);
        }
      })
      .catch(() => {
        if (current) setCatalogFailed(true);
      });
    return () => { current = false; };
  }, [apiBase]);

  const status = token.status;
  const selectedAdapter = status?.adapter ?? "";
  const selectedModel = status?.model ?? "";
  const adapters = catalog?.adapters ?? [];
  const selected = adapters.find((adapter) => adapter.id === selectedAdapter);
  const modelOptions = selected?.models ?? [];
  const statusText = stateLabel(status?.state, !status && !token.statusFailed, token.statusFailed);
  const hasToken = status?.token.present === true;

  return (
    <SettingsCard
      icon={<Sparkles size={16} />}
      title="Supporter Agent"
      headerRight={<SettingsPill tone={stateTone(status?.state)}>{statusText}</SettingsPill>}
      description="The Supporter Agent helps CodeTerm find useful memory and read an agent tab's state."
    >
      <div className="flex flex-col gap-2" data-testid="supporter-agent-status" role="status" aria-label="Supporter Agent status">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-[var(--ct-muted-fg)]">
          <span data-testid="supporter-agent-overall-status">{statusText}</span>
          <span data-testid="supporter-agent-token-state">{hasToken ? "Token set" : status ? "No token" : "Token status unknown"}</span>
          <span data-testid="supporter-agent-selection">{selectedAdapter || "No adapter"}{selectedModel ? ` · ${selectedModel}` : ""}</span>
          {status && <span>Deadline {status.deadline_ms} ms</span>}
        </div>
        {token.statusFailed && <p data-testid="supporter-agent-status-failed">Supporter Agent status is unavailable.</p>}
        {token.modeWarning && <p data-testid="supporter-agent-mode-warning">The token file permissions are not owner-only.</p>}
        {token.adapterMissing && <p data-testid="supporter-agent-adapter-missing">The selected adapter is not registered in this build.</p>}
        {catalogFailed && <p data-testid="supporter-agent-models-failed">Available models could not be loaded.</p>}
        {token.error && <p role="alert" data-testid="supporter-agent-error">{token.error}</p>}
      </div>

      <div className="flex flex-col gap-2" data-testid="supporter-agent-features">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <span className="text-xs text-[var(--ct-fg)]">Memory re-rank</span>
            <SettingsPill tone={status?.features.memory_rerank ? "green" : "muted"}>
              {featureState(status?.features.memory_rerank)}
            </SettingsPill>
          </div>
          <Toggle
            label="Memory re-rank"
            checked={status?.features.memory_rerank ?? false}
            onChange={(enabled) => void token.setFeature("memory_rerank", enabled)}
          />
        </div>
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <span className="text-xs text-[var(--ct-fg)]">Tab-state</span>
            <SettingsPill tone={status?.features.tab_state ? "green" : "muted"}>
              {featureState(status?.features.tab_state)}
            </SettingsPill>
          </div>
          <Toggle
            label="Tab-state"
            checked={status?.features.tab_state ?? false}
            onChange={(enabled) => void token.setFeature("tab_state", enabled)}
          />
        </div>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-2" data-testid="supporter-agent-model-selection">
        <FormRow label="Adapter" htmlFor="supporter-agent-adapter">
          <select
            id="supporter-agent-adapter"
            aria-label="Supporter Agent adapter"
            value={selectedAdapter}
            disabled={!adapters.length || token.busy}
            onChange={(event) => void token.select(event.target.value)}
            className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
            data-testid="supporter-agent-adapter-picker"
          >
            {adapters.map((adapter) => <option key={adapter.id} value={adapter.id}>{adapter.id}</option>)}
          </select>
        </FormRow>
        <FormRow label="Model" htmlFor="supporter-agent-model">
          <select
            id="supporter-agent-model"
            aria-label="Supporter Agent model"
            value={selectedModel}
            disabled={!modelOptions.length || token.busy}
            onChange={(event) => void token.select(selectedAdapter, event.target.value)}
            className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
            data-testid="supporter-agent-model-picker"
          >
            {modelOptions.map((model) => <option key={model.id} value={model.id}>{model.display_name}</option>)}
          </select>
        </FormRow>
      </div>

      <form onSubmit={token.saveToken} className="flex flex-col gap-2" data-testid="supporter-agent-token-form">
        <FormRow
          label={hasToken ? "Replace token" : "API token"}
          htmlFor="supporter-agent-token-input"
          hint="Write-only. The token is sent to the local daemon and never shown again."
        >
          <input
            id="supporter-agent-token-input"
            ref={token.tokenRef}
            type="password"
            autoComplete="off"
            spellCheck={false}
            defaultValue=""
            aria-label={hasToken ? "Replace token" : "API token"}
            className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
            data-testid="supporter-agent-token-input"
          />
        </FormRow>
        <div className="flex items-center gap-2">
          <Button type="submit" variant="accent" size="sm" disabled={token.busy} data-testid="supporter-agent-token-save">
            {hasToken ? "Replace token" : "Save token"}
          </Button>
          <Button type="button" variant="ghost" size="sm" disabled={!hasToken || token.busy} onClick={() => void token.clearToken()} data-testid="supporter-agent-token-clear">
            Clear token
          </Button>
        </div>
      </form>

      <div className="flex flex-wrap items-center gap-4" data-testid="supporter-agent-help">
        <InlineLinkButton label="How it works" onClick={() => onOpenGuide("supporter-agent")} testId="supporter-agent-guide-link" />
        <AboutLink href={DOCS_URL} label="TypeSafe docs" Icon={ExternalLink} />
      </div>
    </SettingsCard>
  );
}
