import { Sparkles } from "lucide-react";
import { useState } from "react";
import { configureWithAI } from "../../lib/pluginConfigure";
import { invoke } from "../../lib/bridge";
import type { DaemonApiConfig } from "../../lib/webApi";
import { Button } from "../shared/Button";
import type { CatalogEntry } from "./pluginCatalog";

export function ConfigureWithAiButton({ entry }: { entry: CatalogEntry }) {
  const help = entry.configHelp;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!entry.installed || entry.pendingReload || !help) return null;

  return (
    <div className="relative">
      <Button
        variant="primary"
        size="sm"
        data-testid="extensions-configure-ai"
        data-plugin-id={entry.id}
        aria-label={`Configure ${entry.displayName} with AI`}
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
        className="inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap"
        title={`Start an AI agent that walks you through setting up ${entry.displayName}`}
      >
        <Sparkles size={13} aria-hidden="true" />
        {busy ? "Starting…" : "Configure with AI"}
      </Button>
      {error && <div role="alert" className="mt-1 max-w-[260px] text-right text-[10.5px] text-[var(--ct-red)] [overflow-wrap:anywhere]">{error}</div>}
    </div>
  );
}
