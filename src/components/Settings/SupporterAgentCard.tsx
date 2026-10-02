import { useEffect, useRef, useState } from "react";
import { RefreshCw, Sparkles } from "lucide-react";
import type { HelpSectionId } from "../../help/types";
import { Button } from "../shared/Button";
import { InlineLinkButton, SettingsCard, SettingsPill } from "./shared";
import {
  SupporterBalanceForm,
  SupporterSpendLine,
  supporterIsPriced,
  supporterTokenTotals,
} from "./SupporterSpendPanel";
import { SupporterConnectionRow } from "./SupporterConnectionRow";
import { SupporterConnectPanel } from "./SupporterConnectPanel";
import { useDecisionToken } from "./useDecisionToken";

interface Props {
  apiBase: string;
  apiHeaders: HeadersInit;
  onOpenGuide: (section: HelpSectionId) => void;
}

export function SupporterAgentCard({ apiBase, apiHeaders, onOpenGuide }: Props) {
  const controls = useDecisionToken(apiBase, apiHeaders);
  const [connecting, setConnecting] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const [focusAdapter, setFocusAdapter] = useState<string | null>(null);
  const host = useRef<HTMLDivElement>(null);
  const add = useRef<HTMLDivElement>(null);
  const status = controls.status;
  const rows = [...(status?.connections ?? [])].sort((a, b) => Number(b.active) - Number(a.active));
  const available = controls.catalog.filter((adapter) => !rows.some((row) => row.adapter === adapter.id));
  useEffect(() => {
    if (!focusAdapter) return;
    const row = Array.from(host.current?.querySelectorAll<HTMLElement>("[data-adapter]") ?? []).find(
      (row) => row.dataset.adapter === focusAdapter,
    );
    if (row) {
      row.focus();
      setFocusAdapter(null);
    }
  }, [focusAdapter, status]);
  const finish = (adapter?: string, active?: boolean) => {
    setConnecting(false);
    if (adapter) {
      setFocusAdapter(adapter);
      setAnnouncement(
        active
          ? `${adapter} is active. Previous connections are saved.`
          : `${adapter} saved, not active. Retry activation from its row.`,
      );
    } else add.current?.querySelector("button")?.focus();
  };
  const disabled = controls.busy || controls.statusFailed;
  const overall = controls.statusFailed
    ? status
      ? "Status stale"
      : "Status unavailable"
    : !status
      ? "Checking status"
      : !status.adapter
        ? "Stopped"
        : status.connected
          ? "Active"
          : "Connection needs attention";
  const when = (seconds: number) => new Date(seconds * 1000).toLocaleTimeString();
  return (
    <SettingsCard
      icon={<Sparkles size={16} />}
      title="Supporter Agent"
      headerRight={
        <SettingsPill tone={controls.statusFailed ? "amber" : status?.connected ? "green" : "muted"}>
          {overall}
        </SettingsPill>
      }
      description="Helps CodeTerm pick useful memory and read what an agent tab is doing."
    >
      <div ref={host} className="flex flex-col gap-2">
        <p role="status" aria-live="polite" className="sr-only">
          {announcement}
        </p>
        {controls.statusFailed && (
          <p role="alert">Status is unavailable. Saved rows may be stale; refresh before making changes.</p>
        )}
        {controls.catalogFailed && <p role="alert">The model catalogue is unavailable. Refresh to choose a model.</p>}
        <Button
          variant="ghost"
          size="sm"
          disabled={controls.busy}
          onClick={() => void controls.refreshStatus()}
          aria-label="Refresh Supporter status and models"
        >
          <RefreshCw size={12} aria-hidden="true" /> Refresh
        </Button>
        {rows.map((row) => (
          <SupporterConnectionRow
            key={row.adapter}
            row={row}
            controls={controls}
            removed={() => {
              setAnnouncement(`${row.adapter} removed.${row.active ? " The Supporter stopped." : ""}`);
              add.current?.querySelector("button")?.focus();
            }}
          >
            {row.active && status && (
              <div
                role="status"
                aria-label="Supporter activity"
                className="flex flex-col gap-1 text-xs text-[var(--ct-muted-fg)]"
              >
                <span>
                  {status.calls_last_hour.toLocaleString()} requests in the last hour ·{" "}
                  {status.calls_last_day.toLocaleString()} in the last day
                </span>
                {supporterIsPriced(row.pricing) && <SupporterSpendLine spend={status.spend} />}
                {status.state === "budget_reached" && <span>Request cap reached.</span>}
                {status.budget.free_at !== null && <span>Available again: {when(status.budget.free_at)}</span>}
                <details>
                  <summary className="cursor-pointer">Details</summary>
                  <div className="flex flex-col gap-1">
                    <span>
                      Last activity: {status.last_activity === null ? "none yet" : when(status.last_activity)}
                    </span>
                    <span>{supporterTokenTotals("Today", status.spend.today)}</span>
                    <span>{supporterTokenTotals("This month", status.spend.month)}</span>
                    <span>Answer deadline: {status.deadline_ms} ms</span>
                    {(["rerank", "tab_state"] as const).map((kind) => {
                      const verdict = status.last_verdict[kind];
                      return verdict ? (
                        <span key={kind}>
                          {kind}: {verdict.label ?? "relevance scored"}
                          {verdict.confidence === null ? "" : ` (${Math.round(verdict.confidence * 100)}%)`}
                        </span>
                      ) : null;
                    })}
                    {supporterIsPriced(row.pricing) && (
                      <SupporterBalanceForm
                        adapter={row.adapter}
                        spend={status.spend}
                        busy={disabled}
                        setBalance={(balance) => controls.setBalance(row.adapter, balance)}
                      />
                    )}
                  </div>
                </details>
              </div>
            )}
          </SupporterConnectionRow>
        ))}
        {!status?.adapter && status && (
          <p className="text-xs">The Supporter is stopped. Deterministic behavior is available.</p>
        )}
        {connecting ? (
          <SupporterConnectPanel adapters={available} busy={disabled} connect={controls.connect} finish={finish} />
        ) : (
          <div ref={add}>
            <Button
              variant="ghost"
              size="sm"
              disabled={disabled || !status || controls.catalogFailed || !available.length}
              onClick={() => setConnecting(true)}
            >
              + Connect another adapter
            </Button>
          </div>
        )}
        {Object.entries(controls.errors)
          .filter(([adapter]) => !rows.some((row) => row.adapter === adapter))
          .map(([adapter, error]) => (
            <p key={adapter} role="alert">
              {error.message}
            </p>
          ))}
        <InlineLinkButton
          label="How it works"
          onClick={() => onOpenGuide("supporter-agent")}
          testId="supporter-agent-guide-link"
        />
      </div>
    </SettingsCard>
  );
}
