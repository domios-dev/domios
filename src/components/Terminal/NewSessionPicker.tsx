import { useCallback, useEffect, useMemo, useState } from "react";
import type { AppConfig } from "../../lib/types";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { useMultiplexer } from "../../store/multiplexer";
import { setProviderSettings } from "../../lib/clientSettings";
import {
  defaultNewSessionChoice,
  readPersistedNewSessionChoice,
  writePersistedNewSessionChoice,
  type NewSessionChoice,
  type NewSessionMode,
} from "../../lib/newSessionChoice";
import { clearSessionPickerPane } from "../../lib/sessionPickerPanes";
import { probeProviderInstallMap } from "../../lib/providerInstallStatus";
import { OrchestrationPairForm } from "../Sidebar/OrchestrationPairForm";
import { OrchestrationRoleRow } from "../Sidebar/OrchestrationPairForm";
import { Segmented, type SegmentedOption } from "../shared/Segmented";

const MODE_OPTIONS: SegmentedOption<NewSessionMode>[] = [
  { value: "single", label: "Single agent" },
  { value: "pair", label: "Orchestrator + Planner" },
];

export function NewSessionPicker({
  paneId,
  workspaceId,
  tabId,
  workspaceCwd,
  agentsConfig,
}: {
  paneId: string;
  workspaceId: string;
  tabId: string;
  workspaceCwd?: string | null;
  agentsConfig?: AppConfig["agents"];
}) {
  const { providers } = useAgentProviders();
  const { spawnAgentTab, createOrchestrationPair, closeTab, addNotification } = useMultiplexer();
  const visibleProviders = useMemo(
    () => providers.filter((provider) => agentsConfig?.[provider.id]?.visible_in_menu !== false),
    [providers, agentsConfig],
  );
  const [installMap, setInstallMap] = useState<Record<string, boolean>>({});
  const [choice, setChoice] = useState<NewSessionChoice>(() => defaultNewSessionChoice(visibleProviders));
  const [busy, setBusy] = useState(false);
  const [claudeModalOpen, setClaudeModalOpen] = useState(false);
  const [claudeSkipPerms, setClaudeSkipPerms] = useState(
    () => agentsConfig?.claude?.spawn_skip_permissions === true,
  );
  const [claudeRememberChoice, setClaudeRememberChoice] = useState(false);
  const disabledProviderIds = useMemo(
    () => new Set(visibleProviders.filter((p) => installMap[p.id] === false).map((p) => p.id)),
    [visibleProviders, installMap],
  );

  useEffect(() => {
    if (visibleProviders.length === 0) return;
    setChoice(readPersistedNewSessionChoice(visibleProviders));
  }, [visibleProviders]);

  useEffect(() => {
    let cancelled = false;
    void probeProviderInstallMap(visibleProviders).then((map) => {
      if (!cancelled) setInstallMap(map);
    });
    return () => {
      cancelled = true;
    };
  }, [visibleProviders]);

  const finishSpawn = useCallback(async (runner: () => Promise<void>) => {
    setBusy(true);
    try {
      await runner();
      clearSessionPickerPane(paneId);
      await closeTab(workspaceId, tabId);
    } catch (e) {
      addNotification({
        pane_id: paneId,
        workspace_id: workspaceId,
        title: "Session start failed",
        body: e instanceof Error ? e.message : String(e),
        level: "Error",
        sticky: true,
      });
    } finally {
      setBusy(false);
    }
  }, [addNotification, closeTab, paneId, tabId, workspaceId]);

  const runSingleSpawn = useCallback(async (skipPermissions: boolean) => {
    if (choice.mode !== "single") return;
    if (disabledProviderIds.has(choice.providerId)) return;
    writePersistedNewSessionChoice(choice);
    await finishSpawn(async () => {
      await spawnAgentTab(
        workspaceId,
        choice.providerId,
        skipPermissions,
        workspaceCwd ?? undefined,
        choice.modelId,
      );
    });
  }, [choice, disabledProviderIds, finishSpawn, spawnAgentTab, workspaceCwd, workspaceId]);

  const runPairSpawn = useCallback(async () => {
    if (choice.mode !== "pair") return;
    if (
      disabledProviderIds.has(choice.orchestratorProviderId)
      || disabledProviderIds.has(choice.plannerProviderId)
    ) {
      return;
    }
    writePersistedNewSessionChoice(choice);
    await finishSpawn(async () => {
      await createOrchestrationPair({
        orchestratorAgent: choice.orchestratorProviderId,
        orchestratorModel: choice.orchestratorModelId,
        plannerAgent: choice.plannerProviderId,
        plannerModel: choice.plannerModelId,
        workspaceId,
      });
    });
  }, [choice, createOrchestrationPair, disabledProviderIds, finishSpawn, workspaceId]);

  const handleStart = useCallback(() => {
    if (choice.mode === "pair") {
      void runPairSpawn();
      return;
    }
    const claudeSkipRemembered = agentsConfig?.claude?.spawn_skip_permissions_remembered === true;
    const claudeSavedSkipPerms = agentsConfig?.claude?.spawn_skip_permissions === true;
    if (choice.providerId === "claude" && !claudeSkipRemembered) {
      setClaudeSkipPerms(claudeSavedSkipPerms);
      setClaudeModalOpen(true);
      return;
    }
    void runSingleSpawn(claudeSavedSkipPerms);
  }, [agentsConfig?.claude, choice, runPairSpawn, runSingleSpawn]);

  const handleClaudeConfirm = useCallback(() => {
    if (claudeRememberChoice) {
      void setProviderSettings("claude", {
        spawn_skip_permissions: claudeSkipPerms,
        spawn_skip_permissions_remembered: true,
      });
    }
    setClaudeModalOpen(false);
    void runSingleSpawn(claudeSkipPerms);
  }, [claudeRememberChoice, claudeSkipPerms, runSingleSpawn]);

  const canStart = choice.mode === "single"
    ? choice.providerId.length > 0 && !disabledProviderIds.has(choice.providerId)
    : choice.orchestratorProviderId.length > 0
      && choice.plannerProviderId.length > 0
      && !disabledProviderIds.has(choice.orchestratorProviderId)
      && !disabledProviderIds.has(choice.plannerProviderId);

  return (
    <div
      data-testid="new-session-picker"
      className="new-session-picker flex h-full w-full items-center justify-center p-6"
      style={{ background: "var(--ct-bg)" }}
    >
      <div className="new-session-picker-card w-full max-w-lg rounded-xl border border-[var(--ct-border)] bg-[var(--ct-surface)] p-5 shadow-lg">
        <div className="mb-4">
          <div className="text-[15px] font-medium text-[var(--ct-fg)]">Start a session</div>
          <div className="mt-1 text-[12px] text-[var(--ct-muted-fg)]">
            Choose a single agent or an Orchestrator + Planner pair. Your last choice is remembered.
          </div>
        </div>

        <div className="mb-4">
          <Segmented
            data-testid="new-session-mode-switch"
            value={choice.mode}
            options={MODE_OPTIONS}
            onChange={(mode) => {
              setChoice((prev) => {
                if (prev.mode === mode) return prev;
                if (mode === "single") {
                  return {
                    mode: "single",
                    providerId: prev.mode === "pair" ? prev.orchestratorProviderId : prev.providerId,
                    modelId: prev.mode === "pair" ? prev.orchestratorModelId : prev.modelId,
                  };
                }
                return {
                  mode: "pair",
                  orchestratorProviderId: prev.mode === "single" ? prev.providerId : prev.orchestratorProviderId,
                  orchestratorModelId: prev.mode === "single" ? prev.modelId : prev.orchestratorModelId,
                  plannerProviderId: prev.mode === "pair" ? prev.plannerProviderId : prev.providerId,
                  plannerModelId: prev.mode === "pair" ? prev.plannerModelId : prev.modelId,
                };
              });
            }}
          />
        </div>

        {choice.mode === "single" ? (
          <OrchestrationRoleRow
            label="Agent"
            providers={visibleProviders}
            providerId={choice.providerId}
            modelId={choice.modelId ?? ""}
            disabledProviderIds={disabledProviderIds}
            onProviderChange={(providerId) => {
              setChoice({
                mode: "single",
                providerId,
                modelId: visibleProviders.find((p) => p.id === providerId)?.default_model,
              });
            }}
            onModelChange={(modelId) => setChoice({ ...choice, modelId })}
          />
        ) : (
          <OrchestrationPairForm
            providers={visibleProviders}
            values={{
              orchestratorProviderId: choice.orchestratorProviderId,
              orchestratorModelId: choice.orchestratorModelId ?? "",
              plannerProviderId: choice.plannerProviderId,
              plannerModelId: choice.plannerModelId ?? "",
            }}
            disabledProviderIds={disabledProviderIds}
            onChange={(values) => setChoice({
              mode: "pair",
              orchestratorProviderId: values.orchestratorProviderId,
              orchestratorModelId: values.orchestratorModelId || undefined,
              plannerProviderId: values.plannerProviderId,
              plannerModelId: values.plannerModelId || undefined,
            })}
            layout="card"
          />
        )}

        <div className="mt-5 flex justify-end">
          <button
            type="button"
            data-testid="new-session-start"
            disabled={!canStart || busy}
            onClick={handleStart}
            className="px-4 py-2 text-[12px] font-medium text-white bg-[var(--ct-accent)] rounded-md hover:opacity-90 disabled:opacity-40 transition-opacity"
          >
            {busy ? "Starting…" : "Start session"}
          </button>
        </div>
      </div>

      {claudeModalOpen && (
        <>
          <div className="fixed inset-0 z-50 bg-black/40" onClick={() => setClaudeModalOpen(false)} />
          <div className="fixed z-50 top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-lg shadow-2xl w-[320px] overflow-hidden">
            <div className="px-4 pt-4 pb-2">
              <div className="text-[13px] font-medium text-[var(--ct-fg)]">Spawn Claude</div>
            </div>
            <div className="px-4 py-3 flex flex-col gap-2.5">
              <label className="flex items-center gap-2.5 cursor-pointer">
                <input
                  type="checkbox"
                  checked={claudeSkipPerms}
                  onChange={(e) => setClaudeSkipPerms(e.target.checked)}
                  className="w-3.5 h-3.5 rounded accent-[var(--ct-accent)]"
                />
                <span className="text-[12px] text-[var(--ct-fg)]">Skip permission prompts</span>
              </label>
              <label className="flex items-center gap-2.5 cursor-pointer">
                <input
                  type="checkbox"
                  checked={claudeRememberChoice}
                  onChange={(e) => setClaudeRememberChoice(e.target.checked)}
                  className="w-3.5 h-3.5 rounded accent-[var(--ct-accent)]"
                />
                <span className="text-[12px] text-[var(--ct-muted-fg)]">Remember my choice</span>
              </label>
            </div>
            <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-[var(--ct-border)]">
              <button
                onClick={() => setClaudeModalOpen(false)}
                className="px-3 py-1.5 text-[11px] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] rounded-md hover:bg-[var(--ct-hover)] transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleClaudeConfirm}
                className="px-4 py-1.5 text-[11px] font-medium text-white bg-[var(--ct-accent)] rounded-md hover:opacity-90 transition-opacity"
              >
                Launch
              </button>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
