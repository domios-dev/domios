import { useCallback, useEffect, useState } from "react";
import { Cable } from "lucide-react";
import {
  checkProviderIntegration,
  installProviderIntegration,
  uninstallProviderIntegration,
  type ProviderIntegrationHealth,
} from "../../lib/bridge";
import type { ProviderDescriptor } from "../../lib/types";
import { Button } from "../shared/Button";
import { SettingsCard, SettingsPill } from "./shared";

export type ProviderIntegrationState =
  | "checking"
  | "healthy"
  | "absent"
  | "stale-version"
  | "untrusted-hash"
  | "needs-repair"
  | "unknown"
  | "error";

export function providersWithIntegrationHealth(
  providers: readonly ProviderDescriptor[],
): ProviderDescriptor[] {
  return providers.filter((provider) => provider.has_hooks);
}

export function providerIntegrationState(
  health: ProviderIntegrationHealth | null,
): Exclude<ProviderIntegrationState, "checking" | "error"> {
  if (!health) return "unknown";
  if (health.integration_state === "absent") return "absent";
  if (health.integration_state === "stale_version") return "stale-version";
  if (health.integration_state === "untrusted_hash") return "untrusted-hash";
  if (health.integration_state === "installed") return "healthy";
  if (typeof health.session_start_hook === "boolean") {
    const stopHookMissing = health.stop_hook === false;
    return health.session_start_hook && !stopHookMissing ? "healthy" : "needs-repair";
  }
  if (typeof health.hooks_installed === "boolean") {
    return health.hooks_installed ? "healthy" : "needs-repair";
  }
  if (typeof health.plugin_installed === "boolean") {
    const current = health.plugin_file_current !== false;
    return health.plugin_installed && current ? "healthy" : "needs-repair";
  }
  return health.status === "ok" ? "healthy" : "unknown";
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function ProviderIntegrationHealthCard({
  provider,
}: {
  provider: Pick<ProviderDescriptor, "id" | "display_name">;
}) {
  const [health, setHealth] = useState<ProviderIntegrationHealth | null>(null);
  const [phase, setPhase] = useState<ProviderIntegrationState>("checking");
  const [action, setAction] = useState<"repair" | "uninstall" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const applyHealth = useCallback((next: ProviderIntegrationHealth) => {
    setHealth(next);
    setPhase(providerIntegrationState(next));
  }, []);

  const refresh = useCallback(async () => {
    setPhase("checking");
    setError(null);
    try {
      applyHealth(await checkProviderIntegration(provider.id));
    } catch (nextError) {
      setPhase("error");
      setError(errorMessage(nextError));
    }
  }, [applyHealth, provider.id]);

  useEffect(() => {
    let active = true;
    setPhase("checking");
    setError(null);
    void checkProviderIntegration(provider.id).then((next) => {
      if (active) applyHealth(next);
    }).catch((nextError) => {
      if (!active) return;
      setPhase("error");
      setError(errorMessage(nextError));
    });
    return () => {
      active = false;
    };
  }, [applyHealth, provider.id]);

  const repair = useCallback(async () => {
    setAction("repair");
    setError(null);
    try {
      await installProviderIntegration(provider.id, health?.precompact_hook);
      applyHealth(await checkProviderIntegration(provider.id));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setAction(null);
    }
  }, [applyHealth, health?.precompact_hook, provider.id]);

  const uninstall = useCallback(async () => {
    setAction("uninstall");
    setError(null);
    try {
      await uninstallProviderIntegration(provider.id);
      applyHealth(await checkProviderIntegration(provider.id));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setAction(null);
    }
  }, [applyHealth, provider.id]);

  const busy = action !== null || phase === "checking";
  const staleSessionStart = health?.session_start_hook === false;
  const staleStopHook = health?.stop_hook === false;
  const staleHooks = [
    ...(staleSessionStart ? ["session-start"] : []),
    ...(staleStopHook ? ["stop"] : []),
  ];

  return (
    <SettingsCard
      icon={<Cable size={16} />}
      title={`${provider.display_name} hooks`}
      headerRight={<IntegrationStatePill state={phase} />}
      description="CodeTerm reads the provider's installed hook configuration directly."
    >
      <div
        aria-live="polite"
        data-provider-integration={provider.id}
        data-provider-integration-state={phase}
        className="space-y-2.5"
      >
        {(phase === "needs-repair" || phase === "absent" || phase === "stale-version" || phase === "untrusted-hash") && (
          <div
            role="alert"
            data-provider-integration-stale-hooks={staleHooks.join(",")}
            data-provider-integration-action-required={health?.repair_requires_consent === true ? "explicit-consent" : undefined}
            className="rounded border border-amber-500/40 bg-amber-500/10 p-2.5 text-xs text-[var(--ct-fg)]"
          >
            {phase === "absent"
              ? "The integration is absent. Install it before starting a new agent session."
              : phase === "stale-version"
                ? "The integration is from an older release. Refresh it before starting a new agent session."
                : phase === "untrusted-hash"
                  ? "Codex does not trust this hook content. Grant trust before starting a new agent session."
                  : staleSessionStart
              ? "The session-start hook is missing or stale. Repair the integration before starting a new agent session."
              : staleStopHook
                ? "The stop hook is missing or stale. Repair the integration to restore agent stop callbacks."
                : "The provider hook integration is incomplete and needs repair."}
          </div>
        )}

        {health?.target && (
          <div className="flex items-center justify-between gap-3 text-[10px]">
            <span className="text-[var(--ct-muted)]">Configuration</span>
            <code className="truncate text-[var(--ct-muted-fg)]">{health.target}</code>
          </div>
        )}

        {error && (
          <div role="alert" className="text-[10px] text-[var(--ct-red)]">
            {error}
          </div>
        )}

        <div className="flex items-center justify-end gap-2">
          <Button
            size="compact"
            onClick={() => void refresh()}
            disabled={busy}
            aria-label={`Re-check ${provider.display_name} integration`}
            data-provider-integration-action="refresh"
            className="text-[10px]"
          >
            Re-check
          </Button>
          {phase === "healthy" && (
            <Button
              variant="danger"
              size="compact"
              onClick={() => void uninstall()}
              loading={action === "uninstall"}
              disabled={busy}
              aria-label={`Uninstall ${provider.display_name} integration`}
              data-provider-integration-action="uninstall"
              className="text-[10px]"
            >
              Uninstall
            </Button>
          )}
          {(phase === "needs-repair" || phase === "absent" || phase === "stale-version" || phase === "untrusted-hash") && (
            <Button
              variant="primary"
              size="compact"
              onClick={() => void repair()}
              loading={action === "repair"}
              disabled={busy}
              aria-label={`Repair ${provider.display_name} integration`}
              data-provider-integration-action="repair"
              data-provider-integration-consent="explicit"
              className="text-[10px]"
            >
              {phase === "absent" ? "Install" : phase === "stale-version" ? "Refresh" : phase === "untrusted-hash" ? "Grant trust" : "Repair"}
            </Button>
          )}
        </div>
      </div>
    </SettingsCard>
  );
}

function IntegrationStatePill({ state }: { state: ProviderIntegrationState }) {
  if (state === "healthy") return <SettingsPill tone="green">healthy</SettingsPill>;
  if (state === "absent") return <SettingsPill tone="amber">absent</SettingsPill>;
  if (state === "stale-version") return <SettingsPill tone="amber">stale version</SettingsPill>;
  if (state === "untrusted-hash") return <SettingsPill tone="red">untrusted hook</SettingsPill>;
  if (state === "needs-repair") return <SettingsPill tone="amber">needs repair</SettingsPill>;
  if (state === "error") return <SettingsPill tone="red">check failed</SettingsPill>;
  if (state === "checking") return <SettingsPill>checking</SettingsPill>;
  return <SettingsPill>unknown</SettingsPill>;
}
