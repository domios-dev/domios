import { createNoticePublisher } from "../../lib/notices";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { launchProviderInstaller } from "../../lib/providerInstall";
import type { AppConfig } from "../../lib/types";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { useModelPresets } from "../../hooks/useModelPresets";
import { invoke } from "../../lib/bridge";
import { DEV_BUILD } from "../../lib/devBuild";
import { useMultiplexer, useMuxState } from "../../store/multiplexer";
import { setAgentSettings } from "../../lib/clientSettings";
import {
  allowedNewSessionModes,
  choiceForSessionSection,
  type NewSessionSection,
  defaultNewSessionChoice,
  initialProviderChoice,
  readPersistedNewSessionChoice,
  reconcilePresetSelection,
  resolveEffort,
  resolvePersistedChoice,
  writePersistedNewSessionChoice,
  type NewSessionChoice,
  type NewSessionMode,
} from "../../lib/newSessionChoice";
import { probeProviderInstallMap } from "../../lib/providerInstallStatus";
import { OrchestrationPairForm, OrchestrationRoleRow } from "../Sidebar/OrchestrationPairForm";
import { ModeCards } from "./NewSessionPickerParts";
import { useAnimatedHeight } from "../shared/useAnimatedHeight";
import { Button } from "../shared/Button";
import { X } from "lucide-react";
import { FolderPaneState } from "../shared/FolderPaneState";
import { createPortal } from "react-dom";
import { isFreeLimitRefusal } from "../billing/freeLimitModel";
import {remoteProvider, remoteSpawnSelection, type ExecutionLocation} from "../../lib/remoteExecution";
import {ExecutionLocationPicker} from "./ExecutionLocationPicker";

export function NewSessionPicker({
  workspaceId,
  workspaceCwd,
  agentsConfig,
  agentConfig,
  initialProviderId,
  section = "workspace",
  onClose,
}: {
  workspaceId: string;
  workspaceCwd?: string | null;
  agentsConfig?: AppConfig["agents"];
  agentConfig?: AppConfig["agent"];
  initialProviderId?: string;
  section?: NewSessionSection;
  onClose: () => void;
}) {
  const { providers, loading: providersLoading, refetch: refreshProviders } = useAgentProviders();
  const { spawnAgentTab, createOrchestrationPair, selectTabByTab, upsertToast } = useMultiplexer();
  const state = useMuxState();
  const { presets, loading: presetsLoading, error: presetsError } = useModelPresets();
  const menuProviders = useMemo(
    () => providers.filter((provider) => agentsConfig?.[provider.id]?.visible_in_menu !== false),
    [providers, agentsConfig],
  );
  const visibleProviders = useMemo(() => menuProviders, [menuProviders]);
  const [execution, setExecution] = useState({providerId: "", location: "local" as ExecutionLocation, environmentId: ""});
  const allowedModes = allowedNewSessionModes(section);
  const [installMap, setInstallMap] = useState<Record<string, boolean>>({});
  const [choice, setChoice] = useState<NewSessionChoice>(() => choiceForSessionSection(defaultNewSessionChoice(visibleProviders), section));
  const localId = choice.mode === "single" ? choice.providerId : "";
  const remote = section === "workspace" ? remoteProvider(menuProviders, localId) : undefined;
  const location = remote && execution.providerId === localId ? execution.location : "local";
  const environmentId = execution.providerId === localId ? execution.environmentId : "";
  const [busy, setBusy] = useState(false);
  const [dismissing, setDismissing] = useState(false);
  const [firstAccessFull, setFirstAccessFull] = useState(true);
  const dialogRef = useRef<HTMLDivElement>(null);
  const { frameRef: modeFrameRef, contentRef: modeContentRef } = useAnimatedHeight();
  const disabledProviderIds = useMemo(
    () => new Set(visibleProviders.filter((p) => installMap[p.id] === false).map((p) => p.id)),
    [visibleProviders, installMap],
  );

  const selectedWorkspace = useMemo(
    () => state.workspaces.find((w) => w.id === workspaceId),
    [state.workspaces, workspaceId],
  );

  const hasRestoredRef = useRef(false);
  const remoteRefreshStartedRef = useRef(false);

  const remoteProviderKey = providers
    .filter((provider) => provider.remote_task)
    .map((provider) => provider.id)
    .join("\0");
  const refreshRemoteProviders = useCallback(async () => {
    const remoteProviderIds = remoteProviderKey.split("\0").filter(Boolean);
    await Promise.all(remoteProviderIds.map(async (providerId) => {
      try {
        await invoke("refresh_provider_models", { providerId });
      } catch (error) {
        if (DEV_BUILD) console.debug(`Remote provider model refresh failed for ${providerId}`, error);
        // Keep the cached descriptor visible; the next open or explicit refresh can retry.
      }
    }));
    refreshProviders();
  }, [remoteProviderKey, refreshProviders]);

  useEffect(() => {
    if (providersLoading || remoteRefreshStartedRef.current) return;
    if (!remoteProviderKey) return;
    remoteRefreshStartedRef.current = true;
    void refreshRemoteProviders();
  }, [remoteProviderKey, providersLoading, refreshRemoteProviders]);

  useEffect(() => {
    if (visibleProviders.length === 0) return;
    if (hasRestoredRef.current) {
      setChoice((current) => choiceForSessionSection(resolvePersistedChoice(visibleProviders, current), section));
      return;
    }
    const restored = readPersistedNewSessionChoice(visibleProviders);
    const initialLocalId = initialProviderId;
    if (initialLocalId && visibleProviders.some((provider) => provider.id === initialLocalId)) {
      setChoice(initialProviderChoice(visibleProviders.find((candidate) => candidate.id === initialLocalId)!));
    } else {
      setChoice(choiceForSessionSection(restored, section));
    }
    hasRestoredRef.current = true;
  }, [initialProviderId, visibleProviders, section, menuProviders]);

  useEffect(() => {
    if (!hasRestoredRef.current) return;
    writePersistedNewSessionChoice(choice);
  }, [choice]);

  useEffect(() => {
    if (!presetsLoading && !presetsError) setChoice((current) => reconcilePresetSelection(current, presets));
  }, [presets, presetsLoading, presetsError]);

  useEffect(() => {
    let cancelled = false;
    const probe = () => {
      void probeProviderInstallMap(visibleProviders).then((map) => {
        if (!cancelled) setInstallMap(map);
      });
    };
    probe();
    // Re-probe on refocus so "install in a terminal tab → come back" just works.
    window.addEventListener("focus", probe);
    return () => {
      cancelled = true;
      window.removeEventListener("focus", probe);
    };
  }, [visibleProviders]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || busy) return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [busy, onClose]);

  useEffect(() => {
    const priorFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialogRef.current?.focus();
    return () => priorFocus?.focus();
  }, []);

  const handleInstallProvider = useCallback((providerId: string) => {
    void (async () => {
      try {
        const res = await launchProviderInstaller(providerId);
        const wsId = res.workspace_id ?? state.active_workspace_id;
        if (wsId && res?.tab_id) await selectTabByTab(wsId, res.tab_id).catch(() => {});
      } catch (e) {
        createNoticePublisher({ upsertToast })({ kind: "toast", tabId: null,
          workspaceId: workspaceId,
          title: "Install failed to start",
          detail: e instanceof Error ? e.message : String(e),
          severity: "Error" });
      }
    })();
  }, [upsertToast, selectTabByTab, state.active_workspace_id, workspaceId]);

  const finishSpawn = useCallback(async (
    runner: () => Promise<{ workspaceId: string; tabId: string } | null>,
  ) => {
    setBusy(true);
    try {
      const created = await runner();
      if (created) {
        await selectTabByTab(created.workspaceId, created.tabId).catch(() => {});
      }
    } catch (e) {
      if (isFreeLimitRefusal(e)) return;
      createNoticePublisher({ upsertToast })({ kind: "toast", tabId: null,
        workspaceId: workspaceId,
        title: "Session could not start",
        detail: e instanceof Error ? e.message : String(e),
        severity: "Error" });
    } finally {
      setBusy(false);
    }
  }, [upsertToast, workspaceId, selectTabByTab]);

  const runProviderSpawn = useCallback(async (
    providerId: string,
    modelId: string | undefined,
    effortId?: string,
    preset?: string,
    executionMode?: string,
  ) => {
    if (!executionMode && disabledProviderIds.has(providerId)) return;
    const targetCwd = section === "general" ? workspaceCwd ?? selectedWorkspace?.cwd : selectedWorkspace?.cwd ?? workspaceCwd;
    if (executionMode) {
      await finishSpawn(() => spawnAgentTab(workspaceId, providerId, targetCwd ?? undefined, undefined, undefined, preset, undefined, executionMode));
      return;
    }
    await finishSpawn(() =>
      spawnAgentTab(
        workspaceId,
        providerId,
        targetCwd ?? undefined,
        modelId,
        effortId,
        preset,
        ...(section === "general" ? ["fresh_general" as const] : []),
      ),
    );
  }, [disabledProviderIds, finishSpawn, spawnAgentTab, selectedWorkspace, workspaceId, workspaceCwd, section]);

  const runSingleSpawn = useCallback(async () => {
    if (choice.mode !== "single") return;
    if (location === "cloud") {
      const target = remoteSpawnSelection(menuProviders, choice.providerId, environmentId);
      await runProviderSpawn(target.providerId, undefined, undefined, target.preset, target.executionMode);
      return;
    }
    await runProviderSpawn(choice.providerId, choice.modelId, choice.effortId, choice.preset);
  }, [choice, runProviderSpawn, location, environmentId, menuProviders]);

  const runPairSpawn = useCallback(async () => {
    if (choice.mode !== "pair" || !allowedNewSessionModes(section).includes("pair")) return;
    if (
      disabledProviderIds.has(choice.orchestratorProviderId)
      || disabledProviderIds.has(choice.plannerProviderId)
    ) {
      return;
    }
    await finishSpawn(async () => {
      const result = await createOrchestrationPair({
        orchestratorAgent: choice.orchestratorProviderId,
        orchestratorModel: choice.orchestratorModelId,
        orchestratorReasoningEffort: choice.orchestratorEffortId,
        plannerAgent: choice.plannerProviderId,
        plannerModel: choice.plannerModelId,
        plannerReasoningEffort: choice.plannerEffortId,
        workspaceId,
      });
      return { workspaceId, tabId: result.orchestrator_tab_id };
    });
  }, [choice, createOrchestrationPair, disabledProviderIds, finishSpawn, workspaceId, section]);

  const handleStart = useCallback(() => {
    if (busy || dismissing || selectedWorkspace?.cwd_missing) return;
    setDismissing(true);
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) onClose();
    const start = async () => {
      if (choice.mode === "pair") {
        await runPairSpawn();
      } else {
        await runSingleSpawn();
      }
    };
    if (location === "cloud" || agentConfig?.full_access !== undefined) {
      void start();
      return;
    }
    setBusy(true);
    void setAgentSettings({ full_access: firstAccessFull })
      .then(() => {
        setBusy(false);
        return start();
      })
      .catch((error) => {
        setBusy(false);
        createNoticePublisher({ upsertToast })({ kind: "toast", tabId: null,
          workspaceId: workspaceId,
          title: "Agent access was not saved",
          detail: error instanceof Error ? error.message : String(error),
          severity: "Error" });
      });
  }, [
    upsertToast,
    busy,
    dismissing,
    onClose,
    selectedWorkspace?.cwd_missing,
    agentConfig?.full_access,
    choice.mode,
    firstAccessFull,
    location,
    runPairSpawn,
    runSingleSpawn,
    workspaceId,
  ]);

  const handleModeSelect = useCallback((mode: NewSessionMode) => {
    if (!allowedNewSessionModes(section).includes(mode)) return;
    setChoice((prev) => {
      if (prev.mode === mode) return prev;
      if (mode === "single") {
        return {
          mode: "single",
          providerId: prev.mode === "pair" ? prev.orchestratorProviderId : prev.providerId,
          modelId: prev.mode === "pair" ? prev.orchestratorModelId : prev.modelId,
          effortId: prev.mode === "pair" ? prev.orchestratorEffortId : prev.effortId,
        };
      }
      return {
        mode: "pair",
        orchestratorProviderId: prev.mode === "single" ? prev.providerId : prev.orchestratorProviderId,
        orchestratorModelId: prev.mode === "single" ? prev.modelId : prev.orchestratorModelId,
        orchestratorEffortId: prev.mode === "single" ? prev.effortId : prev.orchestratorEffortId,
        plannerProviderId: prev.mode === "pair" ? prev.plannerProviderId : prev.providerId,
        plannerModelId: prev.mode === "pair" ? prev.plannerModelId : prev.modelId,
        plannerEffortId: prev.mode === "pair" ? prev.plannerEffortId : prev.effortId,
      };
    });
  }, [section]);

  const remoteCanStart = location === "cloud" && remote?.remote_task?.availability.status === "available"
    && remote.remote_task.environments.some(environment => environment.id === environmentId);
  const canStart = choice.mode === "single"
    ? location === "cloud" ? remoteCanStart : choice.providerId.length > 0 && !disabledProviderIds.has(choice.providerId)
    : choice.orchestratorProviderId.length > 0
      && choice.plannerProviderId.length > 0
      && !disabledProviderIds.has(choice.orchestratorProviderId)
      && !disabledProviderIds.has(choice.plannerProviderId);

  return createPortal(
    <div
      data-testid="new-session-picker"
      data-dismissing={dismissing || undefined}
      onAnimationEnd={(event) => { if (dismissing && event.target === event.currentTarget) onClose(); }}
      className="new-session-picker fixed inset-0 z-[1000] flex items-start justify-center overflow-y-auto overflow-x-hidden bg-black/55 p-6 backdrop-blur-sm"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-session-picker-title"
        tabIndex={-1}
        className="new-session-picker-card picker-stagger relative my-auto w-full max-w-lg overflow-hidden rounded-xl border border-[var(--ct-border)] bg-[var(--ct-surface)] p-5 shadow-2xl outline-none"
      >
        <span
          aria-hidden
          className="picker-ambient-wash"
          data-mode={choice.mode}
        />
        <div className="relative mb-4 pr-8">
          <h2 id="new-session-picker-title" className="text-[15px] font-medium text-[var(--ct-fg)]">Start a session</h2>
        </div>
        <button
          type="button"
          data-testid="picker-close"
          aria-label="Close new session dialog"
          disabled={busy}
          onClick={onClose}
          className="absolute right-3 top-3 z-10 flex h-7 w-7 items-center justify-center rounded-md text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] disabled:opacity-40"
        >
          <X aria-hidden size={14} />
        </button>

        {selectedWorkspace?.cwd_missing ? <FolderPaneState workspace={selectedWorkspace} /> : <>
        <div className="relative mb-4" role="group" aria-label="Session mode">
          <ModeCards mode={choice.mode} allowedModes={allowedModes} onSelect={handleModeSelect} />
        </div>

        <div ref={modeFrameRef} className="picker-mode-frame relative">
          <div ref={modeContentRef}>
            <div key={choice.mode} className="picker-mode-panel">
              {choice.mode === "single" && location === "local" && presets.some((preset) => preset.provider === choice.providerId) && <label>
                Preset<select aria-label="Preset" disabled={presetsLoading} value={choice.preset ?? ""} onChange={(event) => {
                  const preset = presets.find((preset) => preset.provider === choice.providerId && preset.name === event.target.value);
                  setChoice({ ...choice, preset: preset?.name, modelId: preset?.model ?? choice.modelId,
                    effortId: resolveEffort(visibleProviders.find((provider) => provider.id === choice.providerId), preset?.model ?? choice.modelId, choice.effortId) });
                }}>
                  <option value="">No preset</option>
                  {presets.filter((preset) => preset.provider === choice.providerId).map((preset) => <option key={preset.name} value={preset.name}>{preset.name}</option>)}
                </select>
              </label>}
              {choice.mode === "single" && <ExecutionLocationPicker provider={remote} location={location} environmentId={environmentId}
                onLocation={next => setExecution({providerId: localId, location: next, environmentId})}
                onEnvironment={id => setExecution({providerId: localId, location, environmentId: id})}
                onRefresh={() => { void refreshRemoteProviders(); }} />}
              {choice.mode === "single" ? (
                <OrchestrationRoleRow
                  label="Agent"
                  modelControls={location === "local"}
                  providers={visibleProviders}
                  providerId={choice.providerId}
                  modelId={choice.modelId ?? ""}
                  effortId={choice.effortId}
                  disabledProviderIds={location === "local" ? disabledProviderIds : undefined}
                  onProviderChange={(providerId) => {
                    const provider = visibleProviders.find((p) => p.id === providerId);
                    if (provider) setChoice(initialProviderChoice(provider));
                  }}
                  onModelChange={(modelId) => setChoice({
                    ...choice,
                    preset: undefined,
                    modelId,
                    effortId: resolveEffort(
                      visibleProviders.find((provider) => provider.id === choice.providerId),
                      modelId,
                      choice.effortId,
                    ),
                  })}
                  onEffortChange={(effortId) => setChoice({ ...choice, effortId })}
                  effortTestId="picker-effort-single"
                  onInstallProvider={handleInstallProvider}
                />
              ) : (
                <>
                  <p data-testid="picker-team-hint" className="mb-2.5 text-[12px] leading-snug text-[var(--ct-muted-fg)]">
                    Tell the Orchestrator what you need. It plans and brings in the agents.
                  </p>
                  <OrchestrationPairForm
                    providers={visibleProviders}
                    values={{
                      orchestratorProviderId: choice.orchestratorProviderId,
                      orchestratorModelId: choice.orchestratorModelId ?? "",
                      orchestratorEffortId: choice.orchestratorEffortId,
                      plannerProviderId: choice.plannerProviderId,
                      plannerModelId: choice.plannerModelId ?? "",
                      plannerEffortId: choice.plannerEffortId,
                    }}
                    disabledProviderIds={disabledProviderIds}
                    onChange={(values) => setChoice({
                      mode: "pair",
                      orchestratorProviderId: values.orchestratorProviderId,
                      orchestratorModelId: values.orchestratorModelId || undefined,
                      orchestratorEffortId: values.orchestratorEffortId,
                      plannerProviderId: values.plannerProviderId,
                      plannerModelId: values.plannerModelId || undefined,
                      plannerEffortId: values.plannerEffortId,
                    })}
                    layout="team"
                    pairAffordanceTestId="pair-affordance"
                    roleTestIds={{
                      orchestratorProvider: "picker-provider-orchestrator",
                      orchestratorModel: "picker-model-orchestrator",
                      orchestratorEffort: "picker-effort-orchestrator",
                      plannerProvider: "picker-provider-planner",
                      plannerModel: "picker-model-planner",
                      plannerEffort: "picker-effort-planner",
                    }}
                    onInstallProvider={handleInstallProvider}
                  />
                </>
              )}
            </div>
          </div>
        </div>

        {location !== "cloud" && agentConfig?.full_access === undefined && (
          <label className="relative mt-4 flex cursor-pointer items-start gap-2.5 rounded-md border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3 py-2.5">
            <input
              type="checkbox"
              checked={firstAccessFull}
              onChange={(event) => setFirstAccessFull(event.target.checked)}
              className="mt-0.5 h-3.5 w-3.5 accent-[var(--ct-accent)]"
            />
            <span>
              <span className="block text-[12px] text-[var(--ct-fg)]">Full access for agents</span>
              <span className="mt-0.5 block text-[11px] text-[var(--ct-muted-fg)]">
                Agents can edit files and run commands without asking. You can change this later in Settings.
              </span>
            </span>
          </label>
        )}

        <div className="relative mt-4 flex justify-end">
          <Button
            variant="primary"
            className="text-xs"
            data-testid="picker-confirm"
            disabled={!canStart}
            loading={busy}
            onClick={handleStart}
          >
            {choice.mode === "pair" ? "Start team" : "Start session"}
          </Button>
        </div>
        </>}
      </div>

    </div>,
    document.body,
  );
}
