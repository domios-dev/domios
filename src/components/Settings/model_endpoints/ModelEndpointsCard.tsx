import { InlineErrorNotice } from "../../shared/ErrorNotice";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Waypoints } from "lucide-react";
import type { DaemonApiConfig } from "../../../lib/webApi";
import { Button } from "../../shared/Button";
import { Chip } from "../../shared/Chip";
import { SettingsCard } from "../shared";
import { ModelEndpointForm } from "./ModelEndpointForm";
import { modelEndpointsApi, type ModelEndpointsApi } from "./modelEndpointsApi";
import { endpointHost, errorLine, modelCountLabel, type ModelEndpoint } from "./modelEndpointsModel";

type Editing = { kind: "none" } | { kind: "add" } | { kind: "edit"; id: string };
type RowNote = { tone: "ok" | "error"; text: string };

export function ModelEndpointsCard({
  providerId,
  harnessName,
  daemonApi,
  api: injected,
}: {
  providerId: string;
  harnessName: string;
  daemonApi?: DaemonApiConfig;
  api?: ModelEndpointsApi;
}) {
  const api = useMemo(() => injected ?? modelEndpointsApi(providerId, daemonApi), [injected, providerId, daemonApi]);
  const [endpoints, setEndpoints] = useState<ModelEndpoint[] | null>(null);
  const [loadError, setLoadError] = useState("");
  const [editing, setEditing] = useState<Editing>({ kind: "none" });
  const [busyRow, setBusyRow] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);
  const [notes, setNotes] = useState<Record<string, RowNote>>({});

  const reload = useCallback(async () => {
    try {
      setEndpoints(await api.list());
      setLoadError("");
    } catch (error) {
      setLoadError(errorLine(error));
      setEndpoints((current) => current ?? []);
    }
  }, [api]);

  useEffect(() => { void reload(); }, [reload]);

  const note = (id: string, value: RowNote | null) =>
    setNotes((current) => {
      const next = { ...current };
      if (value) next[id] = value;
      else delete next[id];
      return next;
    });

  const test = async (id: string) => {
    setBusyRow(id);
    note(id, null);
    try {
      const models = await api.test(id);
      note(id, { tone: "ok", text: `Connected. The endpoint lists ${modelCountLabel(models.length)}.` });
    } catch (error) {
      note(id, { tone: "error", text: errorLine(error) });
    } finally {
      setBusyRow(null);
    }
  };

  const remove = async (id: string) => {
    setBusyRow(id);
    try {
      await api.remove(id);
      setConfirmRemove(null);
      note(id, null);
      await reload();
    } catch (error) {
      note(id, { tone: "error", text: errorLine(error) });
    } finally {
      setBusyRow(null);
    }
  };

  const saved = async (id: string) => {
    setEditing({ kind: "none" });
    note(id, null);
    await reload();
  };

  const list = endpoints ?? [];
  const editingEndpoint = editing.kind === "edit" ? list.find((endpoint) => endpoint.id === editing.id) : undefined;

  return (
    <SettingsCard
      icon={<Waypoints size={16} />}
      title="Custom model endpoints"
      description={`Add any OpenAI-compatible or Anthropic endpoint; its models appear in ${harnessName}'s model picker.`}
      headerRight={editing.kind === "none" && list.length > 0 ? (
        <Button size="compact" variant="primary" onClick={() => setEditing({ kind: "add" })}>Add endpoint</Button>
      ) : undefined}
    >
      <section aria-label="Custom model endpoints" aria-busy={endpoints === null} data-testid="model-endpoints" data-provider-id={providerId} className="flex flex-col gap-2.5">
        {loadError && <InlineErrorNotice error={loadError} operation="loading model endpoints" blocking />}

        {endpoints !== null && list.length === 0 && editing.kind === "none" && (
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-dashed border-[var(--ct-border)] px-3.5 py-3">
            <p className="m-0 min-w-0 flex-1 text-xs text-[var(--ct-muted-fg)]">
              No endpoints yet. Point {harnessName} at LM Studio, OpenRouter, or any server that speaks the OpenAI or Anthropic API.
            </p>
            <Button size="sm" variant="primary" onClick={() => setEditing({ kind: "add" })}>Add endpoint</Button>
          </div>
        )}

        {list.length > 0 && (
          <ul aria-label="Configured endpoints" className="m-0 flex list-none flex-col divide-y divide-[var(--ct-border)] rounded-lg border border-[var(--ct-border)] p-0">
            {list.map((endpoint) => {
              const rowBusy = busyRow === endpoint.id;
              const rowNote = notes[endpoint.id];
              const confirming = confirmRemove === endpoint.id;
              return (
                <li key={endpoint.id} className="flex flex-col gap-1 px-3 py-2.5" data-endpoint-id={endpoint.id}>
                  <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1.5">
                    <div className="flex min-w-0 flex-1 items-baseline gap-2">
                      <span className="ct-text-mono truncate text-xs font-medium text-[var(--ct-fg)]">{endpoint.id}</span>
                      <span className="truncate text-[11px] text-[var(--ct-muted)]" title={endpoint.baseUrl}>{endpointHost(endpoint.baseUrl)}</span>
                    </div>
                    <Chip variant="muted">{modelCountLabel(endpoint.models.length)}</Chip>
                    {confirming ? (
                      <div className="flex items-center gap-1.5">
                        <Button size="compact" variant="ghost" disabled={rowBusy} onClick={() => setConfirmRemove(null)}>Keep</Button>
                        <Button size="compact" variant="danger" loading={rowBusy} aria-label={`Confirm remove ${endpoint.id}`} onClick={() => void remove(endpoint.id)}>
                          Remove
                        </Button>
                      </div>
                    ) : (
                      <div className="flex items-center gap-1.5">
                        <Button size="compact" variant="ghost" loading={rowBusy} aria-label={`Test connection to ${endpoint.id}`} onClick={() => void test(endpoint.id)}>
                          Test
                        </Button>
                        <Button size="compact" variant="ghost" disabled={rowBusy || editing.kind !== "none"} aria-label={`Edit ${endpoint.id}`} onClick={() => setEditing({ kind: "edit", id: endpoint.id })}>
                          Edit
                        </Button>
                        <Button size="compact" variant="ghost" disabled={rowBusy} aria-label={`Remove ${endpoint.id}`} onClick={() => setConfirmRemove(endpoint.id)}>
                          Remove
                        </Button>
                      </div>
                    )}
                  </div>
                  {confirming && (
                    <p className="m-0 text-[11px] text-[var(--ct-muted-fg)]">Removes the endpoint from {harnessName} and deletes its saved key.</p>
                  )}
                  {endpoint.secretName && !confirming && (
                    <p className="m-0 truncate text-[10px] text-[var(--ct-muted)]">
                      Key <span className="ct-text-mono">{endpoint.secretName}</span>
                    </p>
                  )}
                  {rowNote && (
                    rowNote.tone === "error" ? <InlineErrorNotice error={rowNote.text} operation="testing this model endpoint" blocking />
                      : <p role="status" className="m-0 text-[11px] text-[var(--ct-green)]">{rowNote.text}</p>
                  )}
                </li>
              );
            })}
          </ul>
        )}

        {editing.kind === "add" && (
          <ModelEndpointForm api={api} onSaved={(id) => void saved(id)} onCancel={() => setEditing({ kind: "none" })} />
        )}
        {editingEndpoint && (
          <ModelEndpointForm key={editingEndpoint.id} api={api} original={editingEndpoint} onSaved={(id) => void saved(id)} onCancel={() => setEditing({ kind: "none" })} />
        )}
      </section>
    </SettingsCard>
  );
}
