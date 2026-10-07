import { useEffect, useState } from "react";
import { Server } from "lucide-react";
import { SettingsCard } from "./shared";
import { UI_BUTTON, UI_INPUT } from "../shared/design";
import { modelProviderApi, type ModelProviderApi, type ModelProviderEntry, type ModelProviderForm } from "./modelProviderApi";

const emptyForm: ModelProviderForm = { name: "", kind: "openai-compatible", baseUrl: "", sourceSecret: "", models: "" };

export function ModelProvidersPanel({ api = modelProviderApi }: { api?: ModelProviderApi }) {
  const [providers, setProviders] = useState<ModelProviderEntry[]>([]);
  const [form, setForm] = useState(emptyForm);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState("");
  const [supported, setSupported] = useState(true);

  useEffect(() => {
    let active = true;
    void api.list().then((rows) => { if (active) setProviders(rows); }).catch((error: unknown) => {
      if (!active) return;
      if (error === "unsupported") setSupported(false);
      else setError(String(error));
    });
    return () => { active = false; };
  }, [api]);

  async function run(operation: () => Promise<void>) {
    setBusy(true);
    setError("");
    setResult("");
    try {
      await operation();
      setProviders(await api.list());
    } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  }

  if (!supported) return null;
  return <SettingsCard icon={<Server size={16} />} title="Model providers"
    description="Connect model endpoints using keys already saved in Secrets. Import new keys through codeterm model-provider add using stdin.">
    <section aria-label="Model providers" aria-busy={busy} className="flex flex-col gap-3">
      {error && <p role="alert">{error}</p>}
      <p role="status" aria-live="polite">{result}</p>
      <ul aria-label="Configured model providers">
        {providers.map((provider) => <li key={provider.id} className="flex items-center gap-2">
          <span>{provider.name ?? provider.id} — {provider.models.join(", ")}</span>
          <button className={UI_BUTTON.base} disabled={busy} aria-label={`Test ${provider.id}`}
            onClick={() => void run(async () => setResult((await api.test(provider.id)).join(", ")))}>Test</button>
          <button className={UI_BUTTON.base} disabled={busy} aria-label={`Remove ${provider.id}`}
            onClick={() => void run(() => api.remove(provider.id))}>Remove</button>
        </li>)}
      </ul>
      <form aria-label="Add model provider" className="flex flex-col gap-2" onSubmit={(event) => {
        event.preventDefault();
        void run(async () => { await api.add(form); setForm(emptyForm); });
      }}>
        <label>Name<input className={UI_INPUT.base} required value={form.name} pattern="[a-z0-9]+(-[a-z0-9]+)*"
          onChange={(event) => setForm({ ...form, name: event.target.value })} /></label>
        <label>Kind<select className={UI_INPUT.base} value={form.kind}
          onChange={(event) => setForm({ ...form, kind: event.target.value as ModelProviderForm["kind"] })}>
          <option value="openai-compatible">OpenAI compatible</option><option value="anthropic">Anthropic</option>
        </select></label>
        <label>Base URL<input className={UI_INPUT.base} required type="url" value={form.baseUrl}
          onChange={(event) => setForm({ ...form, baseUrl: event.target.value })} /></label>
        <label>Saved secret name<input className={UI_INPUT.base} required value={form.sourceSecret}
          onChange={(event) => setForm({ ...form, sourceSecret: event.target.value })} /></label>
        <label>Models<input className={UI_INPUT.base} value={form.models} placeholder="Leave empty to discover"
          onChange={(event) => setForm({ ...form, models: event.target.value })} /></label>
        <button className={UI_BUTTON.base} disabled={busy} type="submit">Add provider</button>
      </form>
    </section>
  </SettingsCard>;
}
