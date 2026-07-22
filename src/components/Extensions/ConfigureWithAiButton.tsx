import { Sparkles } from "lucide-react";
import { configureWithAI } from "../../lib/pluginConfigure";
import { invoke } from "../../lib/bridge";
import type { DaemonApiConfig } from "../../lib/webApi";
import type { CatalogEntry } from "./pluginCatalog";

export function ConfigureWithAiButton({ entry }: { entry: CatalogEntry }) {
  const help = entry.configHelp;
  if (!entry.installed || entry.pendingReload || !help) return null;

  return (
    <button
      type="button"
      data-testid="extensions-configure-ai"
      data-plugin-id={entry.id}
      onClick={() => {
        void (async () => {
          const cfg = await invoke<{ api?: DaemonApiConfig }>("get_config").catch(
            () => ({}) as { api?: DaemonApiConfig },
          );
          await configureWithAI(entry.id, entry.displayName, help, cfg.api ?? {});
        })();
      }}
      className="inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-[11.5px] hover:bg-[var(--ct-bg-hover)]"
      style={{ borderColor: "var(--ct-border-default)", color: "var(--ct-fg)" }}
      title={`Configure ${entry.displayName} with an AI agent`}
    >
      <Sparkles size={13} style={{ color: "var(--ct-accent)" }} />
      Configure with AI
    </button>
  );
}
