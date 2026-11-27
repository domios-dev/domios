import { useCallback, useEffect, useState } from "react";
import {
  getCloudBootstrapStatus,
  prepareCloudProviderSignIn,
  recordCloudProviderSignInTiming,
} from "../../lib/cloudBootstrap/client";
import {
  buildRemoteRuntimeLoginState,
  isWarmConsoleReady,
  partitionProviderCards,
  providerCardKey,
  resolveRemoteLoginTabId,
  summarizeCreatePhases,
} from "../../lib/cloudBootstrap/model";
import type {
  CloudBootstrapSnapshot,
  PrepareProviderSignInItem,
} from "../../lib/cloudBootstrap/types";
import { RuntimeLoginActions } from "../Terminal/RuntimeLoginActions";

export interface CloudBootstrapPanelProps {
  machineId: string;
  initialSnapshot?: CloudBootstrapSnapshot | null;
  onRefresh?: () => void;
}

/**
 * Composable post-enrollment warm console & per-provider owner sign-in card entry surface.
 * Designed to mount inside Switch's preparing/drawer surface while delegating every sign-in
 * flow directly to Domios's single `RuntimeLoginActions` component.
 */
export function CloudBootstrapPanel({
  machineId,
  initialSnapshot = null,
  onRefresh,
}: CloudBootstrapPanelProps) {
  const [snapshot, setSnapshot] = useState<CloudBootstrapSnapshot | null>(initialSnapshot);
  const [prepared, setPrepared] = useState<Record<string, PrepareProviderSignInItem>>({});
  const [preparingKey, setPreparingKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const next = await getCloudBootstrapStatus(machineId);
      setSnapshot(next);
      setError(null);
      onRefresh?.();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not load bootstrap status");
    }
  }, [machineId, onRefresh]);

  useEffect(() => {
    if (initialSnapshot) setSnapshot(initialSnapshot);
  }, [initialSnapshot]);

  const hasInitialSnapshot = initialSnapshot !== null;
  useEffect(() => {
    if (!hasInitialSnapshot) void refresh();
  }, [hasInitialSnapshot, refresh]);

  const handlePrepare = useCallback(
    async (providerId: string, profileId?: string | null) => {
      const key = providerCardKey({ provider_id: providerId, profile_id: profileId });
      setPreparingKey(key);
      setError(null);
      try {
        void recordCloudProviderSignInTiming(machineId, { started_at_ms: Date.now() }).catch(
          () => undefined,
        );
        const reply = await prepareCloudProviderSignIn(machineId, {
          provider_id: providerId,
          ...(profileId ? { profile_id: profileId } : {}),
        });
        setPrepared((prev) => {
          const next = { ...prev };
          for (const item of reply.providers) {
            next[providerCardKey(item)] = item;
          }
          return next;
        });
      } catch (err) {
        setError(err instanceof Error ? err.message : "Could not prepare provider sign-in");
      } finally {
        setPreparingKey(null);
      }
    },
    [machineId],
  );

  const warm = snapshot?.warm_bootstrap ?? null;
  const ready = isWarmConsoleReady(snapshot);
  const timings = summarizeCreatePhases(snapshot?.create_phases);
  const partitioned = partitionProviderCards(warm?.providers ?? []);

  return (
    <section aria-label="Cloud warm bootstrap" className="flex flex-col gap-3">
      <div role="status" aria-label="First console readiness" data-warm-ready={ready ? "true" : "false"}>
        {ready
          ? warm?.status === "partial"
            ? "First console ready (partial seed)"
            : warm?.status === "skipped_restored_state"
              ? "Restored owner state preserved"
              : "First console ready"
          : warm?.status === "rejected" || warm?.status === "failed"
            ? "First console not ready (owner context unverified)"
            : "Preparing first console"}
      </div>

      {timings.warmBootstrapDeltaSecs != null && (
        <output aria-label="Warm bootstrap duration">
          Warm bootstrap: {timings.warmBootstrapDeltaSecs.toFixed(2)}s
          {timings.withinBudget === false ? " (over 10s budget)" : ""}
        </output>
      )}

      {warm && warm.excluded.length > 0 && (
        <div role="status" aria-label="Sanitized exclusions">
          Excluded {warm.excluded.length} non-portable or secret-shaped item(s).
        </div>
      )}

      {error && <div role="alert">{error}</div>}

      {partitioned.signInRequired.length > 0 && (
        <div role="region" aria-label="Providers requiring owner sign-in" className="flex flex-col gap-2">
          {partitioned.signInRequired.map((provider) => {
            const key = providerCardKey(provider);
            const prep = prepared[key];
            const remoteTabId = resolveRemoteLoginTabId(snapshot?.peer_id ?? "", provider, prep);
            if (remoteTabId) {
              return (
                <RuntimeLoginActions
                  key={key}
                  tabId={remoteTabId}
                  providerId={provider.provider_id}
                  state={buildRemoteRuntimeLoginState(provider)}
                  variant="inline"
                  onCheck={() => {
                    void recordCloudProviderSignInTiming(machineId, {
                      completed_at_ms: Date.now(),
                    }).catch(() => undefined);
                    void refresh();
                  }}
                />
              );
            }
            return (
              <div key={key} role="group" aria-label={`Sign in to ${provider.display_name}`}>
                <span>{provider.display_name}: Sign in required</span>
                <button
                  type="button"
                  disabled={preparingKey === key}
                  onClick={() => void handlePrepare(provider.provider_id, provider.profile_id)}
                >
                  {preparingKey === key ? "Preparing…" : `Sign in to ${provider.display_name}`}
                </button>
              </div>
            );
          })}
        </div>
      )}

      {partitioned.ready.length > 0 && (
        <ul aria-label="Ready providers">
          {partitioned.ready.map((provider) => (
            <li key={providerCardKey(provider)}>{provider.display_name}: Ready</li>
          ))}
        </ul>
      )}

      {partitioned.unsupported.length > 0 && (
        <ul aria-label="Unsupported providers">
          {partitioned.unsupported.map((provider) => (
            <li key={providerCardKey(provider)}>
              {provider.display_name}: Unsupported ({provider.unsupported_reason ?? "unsupported"})
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
