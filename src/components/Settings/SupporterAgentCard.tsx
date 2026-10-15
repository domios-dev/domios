import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useEffect, useRef, useState } from "react";
import { RefreshCw, Sparkles } from "lucide-react";
import type { HelpSectionId } from "../../help/types";
import { Button } from "../shared/Button";
import { InlineLinkButton, SettingsCard, SettingsPill } from "./shared";
import {
  SupporterSpendLine,
  SupporterSpendSummary,
  supporterActivity,
  supporterIsPriced,
} from "./SupporterSpendPanel";
import { SupporterConnectionRow } from "./SupporterConnectionRow";
import { SupporterConnectPanel } from "./SupporterConnectPanel";
import { useDecisionToken, type SupporterConnection, type SupporterStatus } from "./useDecisionToken";

interface Props {
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
  if (selected.token_mode === "required" && selected.token.mode_ok === false) return "Token needs repair";
  if (selected.token_mode === "required" && selected.token.mode_ok !== true) return "Token status unknown";
  if (!selected.token_mode) return "Availability unknown";
  if (status.state === "not_connected" || !status.connected) return "Disconnected";
  if (status.state === "active") return "Ready";
  return "Availability unknown";
}

export function SupporterAgentCard({ apiBase, apiHeaders, onOpenGuide }: Props) {
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
  const orderedRows = selected ? [selected, ...rows.filter((row) => row.adapter !== selected.adapter)] : rows;
  const available = controls.catalog.filter((adapter) => !rows.some((row) => row.adapter === adapter.id));
  const nameFor = (row: SupporterConnection) =>
    row.metadata?.display_name ??
    controls.catalog.find((adapter) => adapter.id === row.adapter)?.metadata?.display_name ??
    row.adapter;
  const selectionLabel = (adapterId: string, modelId: string | null) => {
    const adapter = controls.catalog.find((entry) => entry.id === adapterId);
    const name = adapter?.metadata?.display_name ?? adapterId;
    const model = adapter?.models.find((entry) => entry.id === modelId)?.display_name ?? modelId ?? "Model not reported";
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
        ? `${removed.name} removed. The Spinal Cord stopped with no automatic fallback.`
        : selectionUnchanged
          ? `${removed.name} removed. The current Spinal Cord is unchanged.`
          : `${removed.name} removed. Current Spinal Cord: ${selection ?? "none selected"}.`,
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
          : `${name} saved, not active. Retry activation from its connection menu.`,
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
  return (
    <SettingsCard
      icon={<Sparkles size={16} />}
      title="Spinal Cord"
      headerRight={
        <div className="flex items-center gap-1.5">
          <SettingsPill tone={controls.statusFailed ? "amber" : currentReadiness === "Ready" ? "green" : "muted"}>
            {overall}
          </SettingsPill>
          <Button
            variant="ghost"
            size="sm"
            className="!h-7 !w-7 !min-h-7 !p-0"
            disabled={controls.busy}
            onClick={() => void controls.refreshStatus()}
            aria-label="Refresh Spinal Cord status and models"
            title="Refresh Spinal Cord status and models"
          >
            <RefreshCw size={13} aria-hidden="true" />
          </Button>
        </div>
      }
      description="Helps Domios pick useful memory and read what an agent tab is doing."
    >
      <div ref={host} className="flex flex-col gap-2">
        <p role="status" aria-live="polite" className="sr-only">
          {announcement}
        </p>
        {controls.statusFailed && (
          <InlineErrorNotice error={{ code: "supporter_status_unavailable", raw: "" }} />
        )}
        {controls.catalogFailed && <InlineErrorNotice error={{ code: "supporter_catalogue_unavailable", raw: "" }} />}
        {status?.adapter && !selected ? (
          <InlineErrorNotice error={{ code: "supporter_connection_unverified", raw: "" }} />
        ) : null}
        {!selected && status && !status.adapter && (
          <p className="text-xs text-[var(--ct-muted-fg)]">
            {controls.statusFailed
              ? "Last-known selection: none. Refresh status before relying on this state."
              : "The Spinal Cord is stopped. Deterministic behavior is available."}
          </p>
        )}
        <section aria-label="Spinal Cord connections" className="ct-rows -mx-2">
          {orderedRows.length ? (
            orderedRows.map((row) => {
              const selectedReadiness = selected?.adapter === row.adapter && status ? readiness(status, row) : null;
              return (
                <SupporterConnectionRow
                  key={row.adapter}
                  row={row}
                  controls={controls}
                  stale={controls.statusFailed}
                  unavailableReason={selectedReadiness && selectedReadiness !== "Ready" ? selectedReadiness : undefined}
                  removed={(selection) => {
                    const unchanged = selection?.adapter === status?.adapter && selection?.model === status?.model;
                    const current = selection?.adapter ? selectionLabel(selection.adapter, selection.model) : null;
                    setAnnouncement(
                      row.active && !selection?.adapter
                        ? `${nameFor(row)} removed. The Spinal Cord stopped with no automatic fallback.`
                        : unchanged
                          ? `${nameFor(row)} removed. The current Spinal Cord is unchanged.`
                          : `${nameFor(row)} removed. Current Spinal Cord: ${current ?? "none selected"}.`,
                    );
                    focusAfterRemoval(row.adapter, selection?.adapter ?? null);
                  }}
                  unconfirmedRemoval={() =>
                    setReconciling((old) => ({
                      ...old,
                      [row.adapter]: {
                        name: nameFor(row),
                        active: row.active,
                        adapter: status?.adapter ?? null,
                        model: status?.model ?? null,
                      },
                    }))
                  }
                >
                  {selected?.adapter === row.adapter && status && (
                    <>
                      <span role="status" aria-label="Spinal Cord activity" title={supporterActivity(status.calls_last_hour, status.calls_last_day).title}>
                        {supporterActivity(status.calls_last_hour, status.calls_last_day).text}
                      </span>
                      {supporterIsPriced(row.pricing) && <SupporterSpendSummary spend={status.spend} />}
                      {supporterIsPriced(row.pricing) && (status.spend.exhausted || !status.spend.storage_ready) && (
                        <SupporterSpendLine spend={status.spend} />
                      )}
                    </>
                  )}
                </SupporterConnectionRow>
              );
            })
          ) : (
            <p className="py-2 text-xs text-[var(--ct-muted-fg)]">No saved connections.</p>
          )}
        </section>
        {connecting ? (
          <SupporterConnectPanel adapters={available} busy={disabled} connect={controls.connect} finish={finish} />
        ) : (
          <div ref={add}>
            <InlineLinkButton
              label="+ Connect another adapter"
              onClick={() => setConnecting(true)}
              disabled={disabled || !status || controls.catalogFailed}
              testId="supporter-agent-connect-link"
            />
          </div>
        )}
        {Object.entries(controls.errors)
          .filter(([adapter]) => !rows.some((row) => row.adapter === adapter))
          .map(([adapter, error]) => (
            <div key={adapter}><InlineErrorNotice error={error.message} operation="connecting the Supporter" blocking /></div>
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
