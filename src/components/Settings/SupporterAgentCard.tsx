import { useEffect, useRef, useState } from "react";
import { RefreshCw, Sparkles } from "lucide-react";
import type { HelpSectionId } from "../../help/types";
import type { ApiConfig } from "../../lib/types";
import { ActivityFeed } from "../Supporter/ActivityFeed";
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
import { useDecisionToken, type SupporterConnection, type SupporterStatus } from "./useDecisionToken";

interface Props {
  api: ApiConfig;
  apiBase: string;
  apiHeaders: HeadersInit;
  onOpenGuide: (section: HelpSectionId) => void;
}

function readiness(status: SupporterStatus, selected: SupporterConnection) {
  if (status.state === "budget_reached" || status.budget.reached || status.spend.exhausted) return "Budget exhausted";
  if (status.state === "credit_exhausted") return "Credit exhausted";
  if (status.state === "no_token") return "Token required";
  if (status.state === "token_unusable") return "Token needs repair";
  if (status.state === "adapter_unavailable") return "Adapter unavailable";
  if (selected.error?.code === "no_token") return "Token required";
  if (selected.error?.code === "token_unusable" || selected.error?.code === "authentication_failed") {
    return "Token needs repair";
  }
  if (selected.token_mode === "required" && !selected.token.present) return "Token required";
  if (
    selected.token_mode === "required" &&
    selected.token.mode_ok === false
  ) return "Token needs repair";
  if (selected.token_mode === "required" && selected.token.mode_ok !== true) return "Token status unknown";
  if (!selected.token_mode) return "Availability unknown";
  if (status.state === "not_connected" || !status.connected) return "Disconnected";
  if (status.state === "active") return "Ready";
  return "Availability unknown";
}

export function SupporterAgentCard({ api, apiBase, apiHeaders, onOpenGuide }: Props) {
  const controls = useDecisionToken(apiBase, apiHeaders);
  const [connecting, setConnecting] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const [focusAdapter, setFocusAdapter] = useState<string | null>(null);
  const [focusAdd, setFocusAdd] = useState(false);
  const [reconciling, setReconciling] = useState<
    Record<string, { name: string; active: boolean; adapter: string | null; model: string | null }>
  >({});
  const host = useRef<HTMLDivElement>(null);
  const add = useRef<HTMLDivElement>(null);
  const status = controls.status;
  const rows = status?.connections ?? [];
  const selected = status?.adapter
    ? rows.find((row) => row.adapter === status.adapter && row.active && row.model === status.model) ?? null
    : null;
  const inactiveRows = rows.filter((row) => row.adapter !== selected?.adapter);
  const available = controls.catalog.filter((adapter) => !rows.some((row) => row.adapter === adapter.id));
  const nameFor = (row: SupporterConnection) =>
    row.metadata?.display_name ??
    controls.catalog.find((adapter) => adapter.id === row.adapter)?.metadata?.display_name ??
    row.adapter;
  const modelFor = (row: SupporterConnection) => {
    const model = controls.catalog
      .find((adapter) => adapter.id === row.adapter)
      ?.models.find((entry) => entry.id === row.model);
    return row.model ? (model ? `${model.display_name} (${row.model})` : row.model) : "Model not reported";
  };
  const selectionLabel = (adapterId: string, modelId: string | null) => {
    const adapter = controls.catalog.find((entry) => entry.id === adapterId);
    const name = adapter?.metadata?.display_name ?? adapterId;
    const model =
      adapter?.models.find((entry) => entry.id === modelId)?.display_name ?? modelId ?? "Model not reported";
    return `${name} · ${model}`;
  };
  const focusAfterRemoval = (removedAdapter: string, selectedAdapter: string | null) => {
    const survivor =
      (selectedAdapter && selectedAdapter !== removedAdapter ? selectedAdapter : null) ??
      rows.find((row) => row.adapter !== removedAdapter)?.adapter;
    if (survivor) setFocusAdapter(survivor);
    else {
      setConnecting(false);
      setFocusAdd(true);
    }
  };
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
  useEffect(() => {
    if (focusAdd && !connecting) {
      add.current?.querySelector("button")?.focus();
      setFocusAdd(false);
    }
  }, [focusAdd, connecting]);
  useEffect(() => {
    if (!status || controls.statusFailed || !Object.keys(reconciling).length) return;
    const confirmedRemoval = Object.entries(reconciling).find(
      ([adapter]) => !status.connections.some((row) => row.adapter === adapter),
    );
    setReconciling({});
    if (!confirmedRemoval) return;
    const [removedAdapter, removed] = confirmedRemoval;
    const selectionUnchanged = status.adapter === removed.adapter && status.model === removed.model;
    const selection = status.adapter ? selectionLabel(status.adapter, status.model) : null;
    setAnnouncement(
      removed.active && !status.adapter
        ? `${removed.name} removed. The Supporter stopped with no automatic fallback.`
        : selectionUnchanged
          ? `${removed.name} removed. The current Supporter is unchanged.`
          : `${removed.name} removed. Current Supporter: ${selection ?? "none selected"}.`,
    );
    focusAfterRemoval(removedAdapter, status.adapter);
  }, [status, controls.statusFailed, reconciling, rows, selectionLabel, focusAfterRemoval]);
  const finish = (adapter?: string, active?: boolean) => {
    setConnecting(false);
    if (adapter) {
      setFocusAdapter(adapter);
      const connection = controls.status?.connections.find((row) => row.adapter === adapter);
      const name =
        connection ? nameFor(connection) : controls.catalog.find((entry) => entry.id === adapter)?.metadata?.display_name ?? adapter;
      setAnnouncement(
        active
          ? `${name} is active. Previous connections are saved.`
          : `${name} saved, not active. Retry activation from its row.`,
      );
    } else add.current?.querySelector("button")?.focus();
  };
  const disabled = controls.busy || controls.statusFailed;
  const currentReadiness = !status
    ? "Availability unknown"
      : !status.adapter
        ? "Stopped"
        : selected
          ? readiness(status, selected)
        : "Selection unknown";
  const overall = controls.statusFailed
    ? status
      ? "Status stale"
      : "Status unavailable"
    : !status
      ? "Checking status"
      : currentReadiness;
  const currentName = selected ? nameFor(selected) : null;
  const when = (seconds: number) => new Date(seconds * 1000).toLocaleTimeString();
  return (
    <SettingsCard
      icon={<Sparkles size={16} />}
      title="Supporter Agent"
      headerRight={
        <SettingsPill tone={controls.statusFailed ? "amber" : currentReadiness === "Ready" ? "green" : "muted"}>
          {overall}
        </SettingsPill>
      }
      description="Helps Domios pick useful memory and read what an agent tab is doing."
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
        <section aria-label="Current Supporter" className="flex flex-col gap-2">
          <h3 className="text-sm font-medium text-[var(--ct-fg)]">Current Supporter</h3>
          {!status ? (
            <p>Status is not available yet.</p>
          ) : selected ? (
            <>
              <p className="text-sm">
                {controls.statusFailed ? "Last-known selection" : "Selected"}: {currentName}
              </p>
              <p className="text-xs">Model: {modelFor(selected)}</p>
              <SettingsPill tone={controls.statusFailed ? "amber" : currentReadiness === "Ready" ? "green" : "muted"}>
                {controls.statusFailed ? `Last-known state: ${currentReadiness}` : currentReadiness}
              </SettingsPill>
              <SupporterConnectionRow
                row={selected}
                controls={controls}
                stale={controls.statusFailed}
                removed={(selection) => {
                  const current = selection?.adapter ? selectionLabel(selection.adapter, selection.model) : null;
                  setAnnouncement(
                    selection?.adapter
                      ? `${nameFor(selected)} removed. Current Supporter: ${current}.`
                      : `${nameFor(selected)} removed. The Supporter stopped with no automatic fallback.`,
                  );
                  focusAfterRemoval(selected.adapter, selection?.adapter ?? null);
                }}
                unconfirmedRemoval={() =>
                  setReconciling((old) => ({
                    ...old,
                    [selected.adapter]: {
                      name: nameFor(selected),
                      active: true,
                      adapter: status.adapter,
                      model: status.model,
                    },
                  }))
                }
              >
                {status && (
                  <div
                    role="status"
                    aria-label="Supporter activity"
                    className="flex flex-col gap-1 text-xs text-[var(--ct-muted-fg)]"
                  >
                    <span>
                      {status.calls_last_hour.toLocaleString()} requests in the last hour ·{" "}
                      {status.calls_last_day.toLocaleString()} in the last day
                    </span>
                    {supporterIsPriced(selected.pricing) && <SupporterSpendLine spend={status.spend} />}
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
                        <span>
                          Team heartbeats wake the Orchestrator only for decisions and incidents; healthy, unchanged work
                          stays quiet.
                        </span>
                        <ActivityFeed api={api} embedded />
                        {(["rerank", "tab_state"] as const).map((kind) => {
                          const verdict = status.last_verdict[kind];
                          return verdict ? (
                            <span key={kind}>
                              {kind}: {verdict.label ?? "relevance scored"}
                              {verdict.confidence === null ? "" : ` (${Math.round(verdict.confidence * 100)}%)`}
                            </span>
                          ) : null;
                        })}
                        {supporterIsPriced(selected.pricing) && (
                          <SupporterBalanceForm
                            adapter={selected.adapter}
                            spend={status.spend}
                            busy={disabled}
                            setBalance={(balance) => controls.setBalance(selected.adapter, balance)}
                          />
                        )}
                      </div>
                    </details>
                  </div>
                )}
              </SupporterConnectionRow>
            </>
          ) : status.adapter ? (
            <p role="alert">The selected connection could not be verified. Refresh status before making changes.</p>
          ) : (
            <p className="text-xs">
              {controls.statusFailed
                ? "Last-known selection: none. Refresh status before relying on this state."
                : "The Supporter is stopped. Deterministic behavior is available."}
            </p>
          )}
        </section>
        <section aria-label="Saved inactive connections" className="flex flex-col gap-2">
          <h3 className="text-sm font-medium text-[var(--ct-fg)]">Saved inactive connections</h3>
          {controls.statusFailed && <p>Inactive labels reflect the last-known status.</p>}
          {inactiveRows.length ? (
            inactiveRows.map((row) => (
              <SupporterConnectionRow
                key={row.adapter}
                row={row}
                controls={controls}
                stale={controls.statusFailed}
                removed={(selection) => {
                  const unchanged = selection?.adapter === status?.adapter && selection?.model === status?.model;
                  const current = selection?.adapter ? selectionLabel(selection.adapter, selection.model) : null;
                  setAnnouncement(
                    unchanged
                      ? `${nameFor(row)} removed. The current Supporter is unchanged.`
                      : `${nameFor(row)} removed. Current Supporter: ${current ?? "none selected"}.`,
                  );
                  focusAfterRemoval(row.adapter, selection?.adapter ?? null);
                }}
                unconfirmedRemoval={() =>
                  setReconciling((old) => ({
                    ...old,
                    [row.adapter]: {
                      name: nameFor(row),
                      active: false,
                      adapter: status?.adapter ?? null,
                      model: status?.model ?? null,
                    },
                  }))
                }
              />
            ))
          ) : (
            <p className="text-xs">No saved inactive connections.</p>
          )}
        </section>
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
