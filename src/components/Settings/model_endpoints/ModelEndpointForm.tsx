import { useId, useState, type FormEvent } from "react";
import { Button } from "../../shared/Button";
import { FormRow } from "../../shared/FormRow";
import { Segmented } from "../../shared/Segmented";
import { UI_INPUT } from "../../shared/design";
import type { ModelEndpointsApi } from "./modelEndpointsApi";
import {
  addRequest,
  applyDiscovered,
  discoverPlan,
  draftFromEndpoint,
  draftProblem,
  emptyDraft,
  errorLine,
  managedSecretName,
  selectableSecrets,
  toggleModel,
  updateRequest,
  type EndpointDraft,
  type EndpointKind,
  type KeySource,
  type ModelEndpoint,
} from "./modelEndpointsModel";

const KIND_OPTIONS: { value: EndpointKind; label: string; title: string }[] = [
  { value: "openai-compatible", label: "OpenAI", title: "OpenAI-compatible API" },
  { value: "anthropic", label: "Anthropic", title: "Anthropic Messages API" },
];

const FIELD_LABEL = "text-[11px] font-medium uppercase tracking-wide text-[var(--ct-muted-fg)]";

export function ModelEndpointForm({
  api,
  original,
  onSaved,
  onCancel,
}: {
  api: ModelEndpointsApi;
  original?: ModelEndpoint;
  onSaved: (id: string) => void;
  onCancel: () => void;
}) {
  const mode = original ? "edit" : "add";
  const ids = { name: useId(), url: useId(), key: useId(), secret: useId(), models: useId(), modelsLabel: useId() };
  const [draft, setDraft] = useState<EndpointDraft>(() => (original ? draftFromEndpoint(original) : emptyDraft()));
  const [secrets, setSecrets] = useState<string[] | null>(null);
  const [busy, setBusy] = useState<"save" | "discover" | null>(null);
  const [error, setError] = useState("");
  const [discoverNote, setDiscoverNote] = useState("");

  const patch = (next: Partial<EndpointDraft>) => {
    setDraft((current) => ({ ...current, ...next }));
    setError("");
  };

  const chooseKeySource = (keySource: KeySource) => {
    patch({ keySource, keyValue: "", savedSecret: "" });
    if (keySource === "saved" && secrets === null) {
      void api.savedSecrets().then((names) => setSecrets(selectableSecrets(names))).catch(() => setSecrets([]));
    }
  };

  const discover = async () => {
    const plan = discoverPlan(draft, original);
    if (plan.kind === "blocked") {
      setDiscoverNote(plan.reason);
      return;
    }
    setBusy("discover");
    setError("");
    setDiscoverNote("");
    try {
      const models = plan.kind === "stored" ? await api.test(plan.id) : await api.discover(plan.request);
      setDraft((current) => applyDiscovered(current, models));
      if (models.length === 0) setDiscoverNote("The endpoint lists no models.");
    } catch (nextError) {
      setError(errorLine(nextError));
    } finally {
      setBusy(null);
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const problem = draftProblem(draft, mode);
    if (problem) {
      setError(problem);
      return;
    }
    setBusy("save");
    setError("");
    try {
      if (original) await api.update(updateRequest(draft));
      else await api.add(addRequest(draft));
      setDraft((current) => ({ ...current, keyValue: "" }));
      onSaved(draft.id.trim());
    } catch (nextError) {
      setError(errorLine(nextError));
    } finally {
      setBusy(null);
    }
  };

  const keyOptions: { value: KeySource; label: string }[] = original
    ? [{ value: "keep", label: "Keep saved key" }, { value: "new", label: "Replace key" }]
    : [{ value: "new", label: "New key" }, { value: "saved", label: "Saved secret" }];
  const secretLabel = original?.secretName ?? managedSecretName(draft.id || "<name>");

  return (
    <form
      aria-label={original ? `Edit endpoint ${original.id}` : "Add model endpoint"}
      aria-busy={busy !== null}
      onSubmit={(event) => void submit(event)}
      className="ct-panel-card flex flex-col gap-3.5 p-3.5"
      noValidate
    >
      <div className="grid grid-cols-1 gap-3.5 sm:grid-cols-[minmax(0,1fr)_auto]">
        <FormRow label="Name" htmlFor={ids.name} hint={original ? undefined : "Lowercase, e.g. lm-studio. Shown in the model picker."}>
          {original ? (
            <code id={ids.name} className="ct-text-mono py-1 text-xs text-[var(--ct-fg)]">{original.id}</code>
          ) : (
            <input
              id={ids.name}
              className={UI_INPUT.fullCompact}
              value={draft.id}
              autoComplete="off"
              spellCheck={false}
              placeholder="lm-studio"
              onChange={(event) => patch({ id: event.currentTarget.value.toLowerCase() })}
            />
          )}
        </FormRow>
        <FormRow label="API format">
          <Segmented<EndpointKind>
            aria-label="API format"
            value={draft.kind}
            options={KIND_OPTIONS}
            height={28}
            onChange={(kind) => patch({ kind })}
          />
        </FormRow>
      </div>

      <FormRow label="Base URL" htmlFor={ids.url} hint="The API root that serves /models, often ending in /v1.">
        <input
          id={ids.url}
          className={UI_INPUT.fullMono}
          type="url"
          value={draft.baseUrl}
          autoComplete="off"
          spellCheck={false}
          placeholder="http://localhost:1234/v1"
          onChange={(event) => patch({ baseUrl: event.currentTarget.value })}
        />
      </FormRow>

      <div className="flex flex-col gap-1.5">
        <div className="flex flex-wrap items-center justify-between gap-2">
          {draft.keySource === "keep" ? (
            <span className={FIELD_LABEL}>API key</span>
          ) : (
            <label htmlFor={draft.keySource === "saved" ? ids.secret : ids.key} className={FIELD_LABEL}>API key</label>
          )}
          <Segmented<KeySource>
            aria-label="Key source"
            value={draft.keySource}
            options={keyOptions}
            height={24}
            onChange={chooseKeySource}
          />
        </div>
        {draft.keySource === "new" && (
          <input
            id={ids.key}
            className={UI_INPUT.fullMono}
            type="password"
            autoComplete="new-password"
            spellCheck={false}
            placeholder={original ? "Paste the new key" : "Paste the key"}
            onChange={(event) => patch({ keyValue: event.currentTarget.value })}
          />
        )}
        {draft.keySource === "saved" && (
          <select
            id={ids.secret}
            className={UI_INPUT.fullCompact}
            value={draft.savedSecret}
            onChange={(event) => patch({ savedSecret: event.currentTarget.value })}
          >
            <option value="">{secrets === null ? "Loading saved secrets…" : secrets.length ? "Choose a saved secret" : "No saved secrets"}</option>
            {(secrets ?? []).map((name) => <option key={name} value={name}>{name}</option>)}
          </select>
        )}
        <span className="text-[10px] text-[var(--ct-muted)]">
          {draft.keySource === "keep"
            ? <>Using <span className="ct-text-mono">{secretLabel}</span> from Secrets.</>
            : <>Stored in Secrets as <span className="ct-text-mono">{secretLabel}</span>; never written to the config file.</>}
        </span>
      </div>

      <div role="group" aria-labelledby={ids.modelsLabel} aria-describedby={ids.models} className="flex min-w-0 flex-col gap-1.5">
        <div className="flex items-center justify-between gap-2">
          <span id={ids.modelsLabel} className={FIELD_LABEL}>Models</span>
          <Button size="compact" variant="ghost" loading={busy === "discover"} disabled={busy === "save"} onClick={() => void discover()}>
            Discover models
          </Button>
        </div>
        {draft.discovered && draft.discovered.length > 0 && (
          <ul aria-label="Discovered models" className="m-0 grid max-h-44 list-none grid-cols-1 gap-x-4 gap-y-1 overflow-y-auto p-0 sm:grid-cols-2">
            {draft.discovered.map((model) => (
              <li key={model} className="min-w-0">
                <label className="flex min-w-0 cursor-pointer items-center gap-2 text-xs text-[var(--ct-fg)]">
                  <input
                    type="checkbox"
                    className="accent-[var(--ct-accent)]"
                    checked={draft.selected.includes(model)}
                    onChange={() => setDraft((current) => toggleModel(current, model))}
                  />
                  <span className="ct-text-mono truncate" title={model}>{model}</span>
                </label>
              </li>
            ))}
          </ul>
        )}
        <span id={ids.models} className="text-[10px] text-[var(--ct-muted)]">
          {discoverNote || "Leave everything unchecked to use every model the endpoint lists."}
        </span>
      </div>

      {error && <p role="alert" className="m-0 text-[11px] text-[var(--ct-red)]">{error}</p>}

      <div className="flex items-center justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onCancel} disabled={busy === "save"}>Cancel</Button>
        <Button size="sm" variant="primary" type="submit" loading={busy === "save"} disabled={busy === "discover"}>
          {original ? "Save endpoint" : "Add endpoint"}
        </Button>
      </div>
    </form>
  );
}
