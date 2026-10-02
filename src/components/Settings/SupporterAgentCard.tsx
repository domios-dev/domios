import { useEffect, useRef, useState } from "react";
import { ExternalLink, RefreshCw, Sparkles } from "lucide-react";
import type { HelpSectionId } from "../../help/types";
import type { ApiConfig } from "../../lib/types";
import { ActivityFeed } from "../Supporter/ActivityFeed";
import { Button } from "../shared/Button";
import { FormRow } from "../shared/FormRow";
import { UI_INPUT } from "../shared/design";
import { AboutLink, InlineLinkButton, SettingsCard, SettingsPill } from "./shared";
import { SupporterBalanceForm, SupporterSpendLine, supporterTokenTotals } from "./SupporterSpendPanel";
import { SupporterTokenField } from "./SupporterTokenField";
import { type SupporterAdapter, useDecisionToken } from "./useDecisionToken";

const DOCS_URL = "https://docs.typesafe.ai";

interface Props {
  api: ApiConfig;
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

export function SupporterAgentCard({ api, apiBase, apiHeaders, onOpenGuide }: Props) {
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

  const tokenState =
    tokenMode === "none" ? "No token needed" : hasToken ? "Token set" : status ? "No token" : "Token status unknown";
  const when = (seconds: number) => new Date(seconds * 1000).toLocaleTimeString();

  return (
    <SettingsCard
      icon={<Sparkles size={16} />}
      title="Supporter Agent"
      headerRight={
        <SettingsPill tone={stateTone(status?.state)}>
          <span className="inline-flex items-center gap-1.5">
            {status?.state === "active" && (
              <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full bg-[var(--ct-green)]" />
            )}
            <span data-testid="supporter-agent-overall-status">{statusText}</span>
          </span>
        </SettingsPill>
      }
      description="Helps CodeTerm pick useful memory and read what an agent tab is doing."
    >
      <div data-testid="supporter-agent-status" role="status" aria-label="Supporter Agent status" className="contents">
        {token.statusFailed && (
          <p data-testid="supporter-agent-status-failed" className="text-xs text-[var(--ct-red)]">Supporter Agent status is unavailable.</p>
        )}
        {token.modeWarning && (
          <p data-testid="supporter-agent-mode-warning" className="text-xs text-[var(--ct-yellow,#d97706)]">The token file permissions are not owner-only.</p>
        )}
        {token.adapterMissing && (
          <p data-testid="supporter-agent-adapter-missing" className="text-xs text-[var(--ct-yellow,#d97706)]">The selected adapter is not registered in this build.</p>
        )}
        {catalogFailed && <p data-testid="supporter-agent-models-failed" className="text-xs text-[var(--ct-yellow,#d97706)]">Available models could not be loaded.</p>}
        {token.error && (
          <p role="alert" data-testid="supporter-agent-error" className="text-xs text-[var(--ct-red)]">
            {token.error}
          </p>
        )}
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-2" data-testid="supporter-agent-model-selection">
        <FormRow label="Adapter" htmlFor="supporter-agent-adapter">
          <select
            id="supporter-agent-adapter"
            aria-label="Supporter Agent adapter"
            value={selectedAdapter}
            disabled={!adapters.length || token.busy}
            onChange={(event) => void token.select(event.target.value)}
            className={UI_INPUT.fullCompact}
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
            className={UI_INPUT.fullCompact}
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

      {tokenNeeded ? (
        <SupporterTokenField
          mode={tokenMode === "optional" ? "optional" : "required"}
          hasToken={hasToken}
          busy={token.busy}
          tokenRef={token.tokenRef}
          saveToken={token.saveToken}
          clearToken={token.clearToken}
        />
      ) : null}
      <span className="sr-only" data-testid="supporter-agent-token-state">{tokenState}</span>

      {status && (
        <div
          className="flex flex-col gap-1 rounded-md border border-[var(--ct-border)] px-3 py-2 text-xs text-[var(--ct-muted-fg)]"
          data-testid="supporter-agent-activity"
          role="status"
          aria-label="Supporter activity"
          data-cap={status.budget.calls_per_hour || "none"}
        >
          <div className="flex items-start justify-between gap-2">
            <div className="flex min-w-0 flex-col gap-0.5">
              <span>
                <span className="text-[var(--ct-fg)]">{status.calls_last_hour.toLocaleString()}</span> requests in the last hour ·{" "}
                <span className="text-[var(--ct-fg)]">{status.calls_last_day.toLocaleString()}</span> in the last day
                {status.budget.calls_per_hour ? ` · cap ${status.budget.calls_per_hour} per hour` : ""}
              </span>
              {status.spend && <SupporterSpendLine spend={status.spend} />}
              {status.budget.storage_ready === false && <span role="alert">Supporter budget storage needs attention.</span>}
              {status.budget.free_at !== null && (
                <span className="text-[var(--ct-yellow,#d97706)]">Available again: {when(status.budget.free_at)}</span>
              )}
            </div>
            <Button
              type="button"
              variant="icon"
              size="sm"
              aria-label="Refresh activity"
              title="Refresh activity"
              onClick={() => void token.refreshStatus()}
            >
              <RefreshCw size={12} aria-hidden="true" />
            </Button>
          </div>
          <details className="group">
            <summary className="cursor-pointer select-none text-[11px] text-[var(--ct-muted)] hover:text-[var(--ct-fg)]">
              Details
            </summary>
            <div className="mt-1 flex flex-col gap-0.5">
              <span>Last activity: {status.last_activity === null ? "none yet" : when(status.last_activity)}</span>
              {(["rerank", "tab_state"] as const).map((kind) => {
                const verdict = status.last_verdict[kind];
                return verdict ? (
                  <span key={kind}>
                    Last {kind === "rerank" ? "memory re-rank" : "tab state"}: {verdict.label ?? "relevance scored"}
                    {verdict.confidence === null ? "" : ` (${Math.round(verdict.confidence * 100)}%)`} at {when(verdict.time)}
                  </span>
                ) : null;
              })}
              {status.spend && <span>{supporterTokenTotals("Today", status.spend.today)}</span>}
              {status.spend && <span>{supporterTokenTotals("This month", status.spend.month)}</span>}
              <span>Answer deadline: {status.deadline_ms} ms</span>
              <span>Team heartbeats wake the Orchestrator only for decisions and incidents; healthy, unchanged work stays quiet.</span>
              <ActivityFeed api={api} embedded />
              {status.spend && (
                <SupporterBalanceForm
                  adapter={selectedAdapter}
                  spend={status.spend}
                  busy={token.busy}
                  setBalance={token.setBalance}
                />
              )}
            </div>
          </details>
        </div>
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
