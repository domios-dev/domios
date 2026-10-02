import { useEffect, useRef, useState } from "react";
import { ExternalLink, Sparkles } from "lucide-react";
import type { HelpSectionId } from "../../help/types";
import { Button } from "../shared/Button";
import { FormRow } from "../shared/FormRow";
import { AboutLink, InlineLinkButton, SettingsCard, SettingsPill } from "./shared";
import { SupporterSpendPanel } from "./SupporterSpendPanel";
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
    case "active":
      return "Active";
    case "budget_reached":
      return "Budget reached";
    case "credit_exhausted":
      return "Balance exhausted";
    case "not_connected":
      return "Not connected";
    case "token_unusable":
      return "Token file needs attention";
    case "adapter_unavailable":
      return "Adapter unavailable";
    case "no_token":
      return "No token";
    default:
      return "Status unavailable";
  }
}

function stateTone(state: string | undefined): "green" | "amber" | "accent" | "muted" {
  if (state === "active") return "green";
  if (state === "no_token") return "muted";
  if (
    state === "credit_exhausted" ||
    state === "budget_reached" ||
    state === "token_unusable" ||
    state === "adapter_unavailable"
  )
    return "amber";
  return "muted";
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
        return (await response.json()) as ModelCatalog;
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
    return () => {
      current = false;
    };
  }, [apiBase]);

  const status = token.status;
  const selectedAdapter = status?.adapter ?? "";
  const selectedModel = status?.model ?? "";
  const adapters = catalog?.adapters ?? [];
  const selected = adapters.find((adapter) => adapter.id === selectedAdapter);
  const modelOptions = selected?.models ?? [];
  const statusText = stateLabel(status?.state, !status && !token.statusFailed, token.statusFailed);
  const hasToken = status?.token.present === true;
  const tokenMode = status?.token_mode ?? selected?.token_mode ?? "required";
  const tokenNeeded = tokenMode !== "none";

  return (
    <SettingsCard
      icon={<Sparkles size={16} />}
      title="Supporter Agent"
      headerRight={<SettingsPill tone={stateTone(status?.state)}>{statusText}</SettingsPill>}
      description="The Supporter Agent helps CodeTerm find useful memory and read an agent tab's state."
    >
      <div
        className="flex flex-col gap-2"
        data-testid="supporter-agent-status"
        role="status"
        aria-label="Supporter Agent status"
      >
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-[var(--ct-muted-fg)]">
          <span data-testid="supporter-agent-overall-status">{statusText}</span>
          <span data-testid="supporter-agent-token-state">
            {tokenMode === "none"
              ? "No token needed"
              : hasToken
                ? "Token set"
                : status
                  ? "No token"
                  : "Token status unknown"}
          </span>
          <span data-testid="supporter-agent-selection">
            {selectedAdapter || "No adapter"}
            {selectedModel ? ` · ${selectedModel}` : ""}
          </span>
          {status && <span>Deadline {status.deadline_ms} ms</span>}
        </div>
        {token.statusFailed && (
          <p data-testid="supporter-agent-status-failed">Supporter Agent status is unavailable.</p>
        )}
        {token.modeWarning && (
          <p data-testid="supporter-agent-mode-warning">The token file permissions are not owner-only.</p>
        )}
        {token.adapterMissing && (
          <p data-testid="supporter-agent-adapter-missing">The selected adapter is not registered in this build.</p>
        )}
        {catalogFailed && <p data-testid="supporter-agent-models-failed">Available models could not be loaded.</p>}
        {token.error && (
          <p role="alert" data-testid="supporter-agent-error">
            {token.error}
          </p>
        )}
      </div>

      {status && (
        <div
          className="flex flex-col gap-1 text-xs text-[var(--ct-muted-fg)]"
          data-testid="supporter-agent-activity"
          role="status"
          aria-label="Supporter activity"
        >
          {status.budget.storage_ready === false && <span role="alert">Supporter budget storage needs attention.</span>}
          <span>
            Calls: {status.calls_last_hour} in the last hour · {status.calls_last_day} in the last day
          </span>
          <span>
            Last activity:{" "}
            {status.last_activity === null ? "None yet" : new Date(status.last_activity * 1000).toLocaleString()}
          </span>
          <span>
            Budget: {status.calls_last_hour} / {status.budget.calls_per_hour} calls per hour
          </span>
          {status.budget.free_at !== null && (
            <span>Available again: {new Date(status.budget.free_at * 1000).toLocaleString()}</span>
          )}
          {(["rerank", "tab_state"] as const).map((kind) => {
            const verdict = status.last_verdict[kind];
            return verdict ? (
              <span key={kind}>
                Last {kind === "rerank" ? "re-rank" : "tab-state"}: {verdict.label ?? "relevance scored"}
                {verdict.confidence === null ? "" : ` · ${Math.round(verdict.confidence * 100)}%`} ·{" "}
                {new Date(verdict.time * 1000).toLocaleString()}
              </span>
            ) : null;
          })}
          <Button type="button" variant="ghost" size="sm" onClick={() => void token.refreshStatus()}>
            Refresh activity
          </Button>
        </div>
      )}

      {status?.spend && (
        <SupporterSpendPanel
          adapter={selectedAdapter}
          spend={status.spend}
          busy={token.busy}
          setBalance={token.setBalance}
        />
      )}

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
            <option value="" disabled>
              Select an adapter
            </option>
            {adapters.map((adapter) => (
              <option key={adapter.id} value={adapter.id}>
                {adapter.id}
              </option>
            ))}
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
            {modelOptions.map((model) => (
              <option key={model.id} value={model.id}>
                {model.display_name}
              </option>
            ))}
          </select>
        </FormRow>
      </div>

      {tokenNeeded && (
        <form onSubmit={token.saveToken} className="flex flex-col gap-2" data-testid="supporter-agent-token-form">
          <FormRow
            label={
              tokenMode === "optional"
                ? hasToken
                  ? "Replace token (optional)"
                  : "API token (optional)"
                : hasToken
                  ? "Replace token"
                  : "API token"
            }
            htmlFor="supporter-agent-token-input"
            hint={
              tokenMode === "optional"
                ? "Optional. Write-only; the token is sent to the local daemon and never shown again."
                : "Write-only. The token is sent to the local daemon and never shown again."
            }
          >
            <input
              id="supporter-agent-token-input"
              ref={token.tokenRef}
              type="password"
              autoComplete="off"
              spellCheck={false}
              defaultValue=""
              aria-label={
                tokenMode === "optional"
                  ? hasToken
                    ? "Replace token (optional)"
                    : "API token (optional)"
                  : hasToken
                    ? "Replace token"
                    : "API token"
              }
              className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
              data-testid="supporter-agent-token-input"
            />
          </FormRow>
          <div className="flex items-center gap-2">
            <Button
              type="submit"
              variant="accent"
              size="sm"
              disabled={token.busy}
              data-testid="supporter-agent-token-save"
            >
              {hasToken ? "Replace token" : "Save token"}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              disabled={!hasToken || token.busy}
              onClick={() => void token.clearToken()}
              data-testid="supporter-agent-token-clear"
            >
              Clear token
            </Button>
          </div>
        </form>
      )}

      <div className="flex flex-wrap items-center gap-4" data-testid="supporter-agent-help">
        <InlineLinkButton
          label="How it works"
          onClick={() => onOpenGuide("supporter-agent")}
          testId="supporter-agent-guide-link"
        />
        <AboutLink href={DOCS_URL} label="TypeSafe docs" Icon={ExternalLink} />
      </div>
    </SettingsCard>
  );
}
