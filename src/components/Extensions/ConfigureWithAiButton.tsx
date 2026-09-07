import { Sparkles } from "lucide-react";
import { useState } from "react";
import { configureWithAI } from "../../lib/pluginConfigure";
import { invoke } from "../../lib/bridge";
import type { DaemonApiConfig } from "../../lib/webApi";
import type { CatalogEntry } from "./pluginCatalog";

export function ConfigureWithAiButton({ entry }: { entry: CatalogEntry }) {
  const help = entry.configHelp;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!entry.installed || entry.pendingReload || !help) return null;

  return (
    <div>
      <button
        type="button"
        data-testid="extensions-configure-ai"
        data-plugin-id={entry.id}
        disabled={busy}
        onClick={() => {
          setBusy(true);
          setError(null);
          void (async () => {
            try {
              const cfg = await invoke<{ api?: DaemonApiConfig }>("get_config").catch(
                () => ({}) as { api?: DaemonApiConfig },
              );
              await configureWithAI(entry.id, entry.displayName, help, cfg.api ?? {});
            } catch (failure) {
              setError(failure instanceof Error ? failure.message : String(failure));
            } finally {
              setBusy(false);
            }
          })();
        }}
        className="inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-[11.5px] hover:bg-[var(--ct-bg-hover)]"
        style={{ borderColor: "var(--ct-border-default)", color: "var(--ct-fg)" }}
        title={`Configure ${entry.displayName} with an AI agent`}
      >
        <Sparkles size={13} style={{ color: "var(--ct-accent)" }} />
        {busy ? "Starting…" : "Configure with AI"}
      </button>
      {error && <div role="alert" className="mt-1 text-[10px] text-[var(--ct-red)]">{error}</div>}
    </div>
  );
}
