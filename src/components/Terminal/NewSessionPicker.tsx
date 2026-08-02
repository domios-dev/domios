import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { IS_WEB } from "../../lib/webApi";
import type { AppConfig } from "../../lib/types";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { useMultiplexer, useMuxState } from "../../store/multiplexer";
import { setProviderSettings } from "../../lib/clientSettings";
import {
  defaultNewSessionChoice,
  readPersistedNewSessionChoice,
  writePersistedNewSessionChoice,
  type NewSessionChoice,
  type NewSessionMode,
} from "../../lib/newSessionChoice";
import { buildDestinationPreview, MODE_HELP_TEXT, WORKSPACE_HELP_TEXT } from "../../lib/newSessionPickerContent";
import { probeProviderInstallMap } from "../../lib/providerInstallStatus";
import { AgentIcon } from "../AgentIcon";
import { OrchestrationPairForm, OrchestrationRoleRow } from "../Sidebar/OrchestrationPairForm";
import { ModeCards, WorkspaceRow } from "./NewSessionPickerParts";
import { FloatingTooltip } from "../shared/FloatingTooltip";
import { RemoteFolderPicker } from "../shared/RemoteFolderPicker";
import { Button } from "../shared/Button";
import { X } from "lucide-react";
import { createPortal } from "react-dom";

export function NewSessionPicker({
  workspaceId,
  workspaceCwd,
  agentsConfig,
  onClose,
}: {
  workspaceId: string;
  workspaceCwd?: string | null;
  agentsConfig?: AppConfig["agents"];
  onClose: () => void;
}) {
  const { providers } = useAgentProviders();
  const { spawnAgentTab, createOrchestrationPair, selectTabByTab, addNotification, createWorkspace } = useMultiplexer();
  const state = useMuxState();
  const visibleProviders = useMemo(
    () => providers.filter((provider) => agentsConfig?.[provider.id]?.visible_in_menu !== false),
    [providers, agentsConfig],
  );
  const [installMap, setInstallMap] = useState<Record<string, boolean>>({});
  const [choice, setChoice] = useState<NewSessionChoice>(() => defaultNewSessionChoice(visibleProviders));
  const [busy, setBusy] = useState(false);
  const [permissionModalProviderId, setPermissionModalProviderId] = useState<string | null>(null);
  const [skipPermissions, setSkipPermissions] = useState(false);
  const [rememberPermissionChoice, setRememberPermissionChoice] = useState(false);
  const dialogRef = useRef<HTMLDivElement>(null);
  const disabledProviderIds = useMemo(
    () => new Set(visibleProviders.filter((p) => installMap[p.id] === false).map((p) => p.id)),
    [visibleProviders, installMap],
  );

  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState(workspaceId);
  const [creatingWorkspace, setCreatingWorkspace] = useState(false);
  const [folderPickerOpen, setFolderPickerOpen] = useState(false);
  const [newWorkspaceName, setNewWorkspaceName] = useState("");
  const workspaces = state.workspaces;
  const selectedWorkspace = useMemo(
    () => workspaces.find((w) => w.id === selectedWorkspaceId),
    [workspaces, selectedWorkspaceId],
  );

  const hasRestoredRef = useRef(false);

  useEffect(() => {
    if (visibleProviders.length === 0) return;
    setChoice(readPersistedNewSessionChoice(visibleProviders));
    hasRestoredRef.current = true;
  }, [visibleProviders]);

  useEffect(() => {
    if (!hasRestoredRef.current) return;
    writePersistedNewSessionChoice(choice);
  }, [choice]);

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
      if (event.key !== "Escape" || busy || permissionModalProviderId || folderPickerOpen) return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [busy, permissionModalProviderId, folderPickerOpen, onClose]);

  useEffect(() => {
    const priorFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialogRef.current?.focus();
    return () => priorFocus?.focus();
  }, []);

  const handleInstallProvider = useCallback((providerId: string) => {
    void (async () => {
      try {
        const res = await invoke<{ tab_id: string }>(
          "install_provider_in_tab",
          { providerId },
        );
        const wsId = state.active_workspace_id;
        if (wsId && res?.tab_id) await selectTabByTab(wsId, res.tab_id).catch(() => {});
        addNotification({
          tab_id: null,
          workspace_id: workspaceId,
          title: "Installer running",
          body: "The install command is running in a terminal tab. Come back to Start a session when it finishes.",
          level: "Info",
          sticky: false,
        });
      } catch (e) {
        addNotification({
          tab_id: null,
          workspace_id: workspaceId,
          title: "Install failed to start",
          body: e instanceof Error ? e.message : String(e),
          level: "Error",
          sticky: true,
        });
      }
    })();
  }, [addNotification, selectTabByTab, state.active_workspace_id, workspaceId]);

  const handleStartCreateWorkspace = useCallback(() => {
    if (IS_WEB) {
      setNewWorkspaceName("");
      setCreatingWorkspace(true);
      return;
    }
    setFolderPickerOpen(true);
  }, []);
  const handleSelectWorkspaceFolder = useCallback((folder: string) => {
    setFolderPickerOpen(false);
    void (async () => {
      const basename = folder.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || "Workspace";
      const info = await createWorkspace(basename, folder, { activate: false });
      if (info?.id) setSelectedWorkspaceId(info.id);
    })();
  }, [createWorkspace]);
  const handleCancelCreateWorkspace = useCallback(() => setCreatingWorkspace(false), []);
  const handleConfirmCreateWorkspace = useCallback(async () => {
    const name = newWorkspaceName.trim();
    if (!name) return;
    const info = await createWorkspace(name, undefined, { activate: false });
    setCreatingWorkspace(false);
    if (info?.id) setSelectedWorkspaceId(info.id);
  }, [newWorkspaceName, createWorkspace]);

  const finishSpawn = useCallback(async (
    runner: () => Promise<{ workspaceId: string; tabId: string } | null>,
  ) => {
    setBusy(true);
    try {
      const created = await runner();
      if (created) {
        await selectTabByTab(created.workspaceId, created.tabId).catch(() => {});
        onClose();
      }
    } catch (e) {
      addNotification({
        tab_id: null,
        workspace_id: workspaceId,
        title: "Session start failed",
        body: e instanceof Error ? e.message : String(e),
        level: "Error",
        sticky: true,
      });
    } finally {
      setBusy(false);
    }
  }, [addNotification, onClose, selectTabByTab, workspaceId]);

  const runProviderSpawn = useCallback(async (
    providerId: string,
    modelId: string | undefined,
    skipPermissions: boolean,
  ) => {
    if (disabledProviderIds.has(providerId)) return;
    const targetCwd = selectedWorkspace?.cwd ?? workspaceCwd ?? undefined;
    await finishSpawn(() =>
      spawnAgentTab(
        selectedWorkspaceId,
        providerId,
        skipPermissions,
        targetCwd ?? undefined,
        modelId,
      ),
    );
  }, [disabledProviderIds, finishSpawn, spawnAgentTab, selectedWorkspace, selectedWorkspaceId, workspaceCwd]);

  const runSingleSpawn = useCallback(async (skipPermissions: boolean) => {
    if (choice.mode !== "single") return;
    await runProviderSpawn(choice.providerId, choice.modelId, skipPermissions);
  }, [choice, runProviderSpawn]);

  const runPairSpawn = useCallback(() => {
    if (choice.mode !== "pair") return;
    if (
      disabledProviderIds.has(choice.orchestratorProviderId)
      || disabledProviderIds.has(choice.plannerProviderId)
    ) {
      return;
    }
    const wsId = selectedWorkspaceId;
    onClose();
    void createOrchestrationPair({
      orchestratorAgent: choice.orchestratorProviderId,
      orchestratorModel: choice.orchestratorModelId,
      plannerAgent: choice.plannerProviderId,
      plannerModel: choice.plannerModelId,
      workspaceId: wsId,
    })
      .then((result) => selectTabByTab(wsId, result.orchestrator_tab_id))
      .catch((e) => {
        addNotification({
          tab_id: null,
          workspace_id: wsId,
          title: "Orchestration start failed",
          body: e instanceof Error ? e.message : String(e),
          level: "Error",
          sticky: true,
        });
      });
  }, [addNotification, choice, createOrchestrationPair, disabledProviderIds, onClose, selectTabByTab, selectedWorkspaceId]);

  const handleStart = useCallback(() => {
    if (choice.mode === "pair") {
      void runPairSpawn();
      return;
    }
    const provider = visibleProviders.find((candidate) => candidate.id === choice.providerId);
    const providerConfig = agentsConfig?.[choice.providerId];
    const remembered = providerConfig?.spawn_skip_permissions_remembered === true;
    const saved = providerConfig?.spawn_skip_permissions === true;
    if (provider?.supports_permission_bypass) {
      setSkipPermissions(saved);
      setRememberPermissionChoice(remembered);
      setPermissionModalProviderId(choice.providerId);
      return;
    }
    void runSingleSpawn(provider?.supports_permission_bypass === true && saved);
  }, [agentsConfig, choice, runPairSpawn, runSingleSpawn, visibleProviders]);

  const handlePermissionConfirm = useCallback(() => {
    if (!permissionModalProviderId) return;
    void setProviderSettings(permissionModalProviderId, {
      spawn_skip_permissions: skipPermissions,
      spawn_skip_permissions_remembered: rememberPermissionChoice,
    });
    setPermissionModalProviderId(null);
    void runSingleSpawn(skipPermissions);
  }, [permissionModalProviderId, rememberPermissionChoice, runSingleSpawn, skipPermissions]);

  const handleModeSelect = useCallback((mode: NewSessionMode) => {
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
  }, []);

  const canStart = choice.mode === "single"
    ? choice.providerId.length > 0 && !disabledProviderIds.has(choice.providerId)
    : choice.orchestratorProviderId.length > 0
      && choice.plannerProviderId.length > 0
      && !disabledProviderIds.has(choice.orchestratorProviderId)
      && !disabledProviderIds.has(choice.plannerProviderId);

  const previewText = useMemo(() => {
    const destinationName = selectedWorkspace?.name ?? "…";
    if (choice.mode === "pair") {
      return buildDestinationPreview({ mode: "pair", workspaceName: destinationName });
    }
    const provider = visibleProviders.find((p) => p.id === choice.providerId);
    const model = provider?.available_models?.find((m) => m.id === choice.modelId);
    return buildDestinationPreview({
      mode: "single",
      workspaceName: destinationName,
      singleProviderLabel: provider?.display_name,
      singleModelLabel: model?.display_name,
    });
  }, [choice, selectedWorkspace, visibleProviders]);

  const singleProviderId = choice.mode === "single" ? choice.providerId : (visibleProviders[0]?.id ?? "");
  const orchestratorProviderId = choice.mode === "pair" ? choice.orchestratorProviderId : singleProviderId;
  const plannerProviderId = choice.mode === "pair" ? choice.plannerProviderId : singleProviderId;

  return createPortal(
    <div
      data-testid="new-session-picker"
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
          <div className="mt-1 text-[12px] text-[var(--ct-muted-fg)]">
            Choose a single agent or an Orchestrator + Planner pair. Your last choice is remembered.
          </div>
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

        <div className="relative mb-1.5 flex items-center gap-1.5">
          <span className="text-[11px] font-medium text-[var(--ct-muted-fg)]">Mode</span>
          <FloatingTooltip content={<div>{MODE_HELP_TEXT}</div>} className="picker-help-glyph">
            <span className="picker-help-glyph-mark">?</span>
          </FloatingTooltip>
        </div>

        <div className="relative mb-4">
          <ModeCards
            mode={choice.mode}
            singleProviderId={singleProviderId}
            orchestratorProviderId={orchestratorProviderId}
            plannerProviderId={plannerProviderId}
            onSelect={handleModeSelect}
          />
        </div>

        <div className="relative mb-4">
          <WorkspaceRow
            workspaces={workspaces.map((w) => ({ id: w.id, name: w.name }))}
            selectedWorkspaceId={selectedWorkspaceId}
            onSelectWorkspace={setSelectedWorkspaceId}
            creatingWorkspace={creatingWorkspace}
            newWorkspaceName={newWorkspaceName}
            onNewWorkspaceNameChange={setNewWorkspaceName}
            onStartCreateWorkspace={handleStartCreateWorkspace}
            onConfirmCreateWorkspace={() => void handleConfirmCreateWorkspace()}
            onCancelCreateWorkspace={handleCancelCreateWorkspace}
            helpText={WORKSPACE_HELP_TEXT}
          />
        </div>

        <div className="relative">
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
              onInstallProvider={handleInstallProvider}
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
              pairAffordanceTestId="pair-affordance"
              roleTestIds={{
                orchestratorProvider: "picker-provider-orchestrator",
                orchestratorModel: "picker-model-orchestrator",
                plannerProvider: "picker-provider-planner",
                plannerModel: "picker-model-planner",
              }}
              onInstallProvider={handleInstallProvider}
            />
          )}
        </div>

        <div
          className="relative mt-4 flex items-center gap-2 rounded-md border px-2.5 py-2 text-[11px] text-[var(--ct-muted-fg)]"
          style={{
            borderColor: "color-mix(in srgb, var(--ct-border) 70%, transparent)",
            background: "color-mix(in srgb, var(--ct-bg) 60%, transparent)",
          }}
          data-testid="picker-preview"
        >
          <span className="flex shrink-0 items-center gap-1" aria-hidden>
            {choice.mode === "pair" ? (
              <>
                <AgentIcon providerId={orchestratorProviderId} className="text-[13px]" />
                <span className="opacity-50 text-[10px]">⟷</span>
                <AgentIcon providerId={plannerProviderId} className="text-[13px]" />
              </>
            ) : (
              <AgentIcon providerId={singleProviderId} className="text-[13px]" />
            )}
          </span>
          <span className="min-w-0 truncate">{previewText}</span>
        </div>

        <div className="relative mt-4 flex justify-end">
          <Button
            variant="primary"
            data-testid="picker-confirm"
            disabled={!canStart}
            loading={busy}
            onClick={handleStart}
          >
            {choice.mode === "pair" ? "Start team" : "Start session"}
          </Button>
        </div>
      </div>

      {permissionModalProviderId && (
        <>
          <div className="fixed inset-0 z-[1010] bg-black/40" onClick={() => setPermissionModalProviderId(null)} />
          <div
            role="dialog"
            aria-modal="true"
            aria-label="Provider permission options"
            className="fixed z-[1010] top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-[var(--ct-surface)] border border-[var(--ct-border)] rounded-lg shadow-2xl w-[320px] overflow-hidden"
          >
            <div className="px-4 pt-4 pb-2">
              <div className="text-[13px] font-medium text-[var(--ct-fg)]">
                Spawn {visibleProviders.find((provider) => provider.id === permissionModalProviderId)?.display_name ?? permissionModalProviderId}
              </div>
              <div className="mt-1 text-[11px] text-[var(--ct-muted-fg)]">Provider permissions and sandboxing stay enabled by default.</div>
            </div>
            <div className="px-4 py-3 flex flex-col gap-2.5">
              <label className="flex items-center gap-2.5 cursor-pointer">
                <input
                  type="checkbox"
                  checked={skipPermissions}
                  onChange={(e) => setSkipPermissions(e.target.checked)}
                  className="w-3.5 h-3.5 rounded accent-[var(--ct-accent)]"
                />
                <span className="text-[12px] text-[var(--ct-fg)]">Bypass provider permissions and sandbox</span>
              </label>
              <label className="flex items-center gap-2.5 cursor-pointer">
                <input
                  type="checkbox"
                  checked={rememberPermissionChoice}
                  onChange={(e) => setRememberPermissionChoice(e.target.checked)}
                  className="w-3.5 h-3.5 rounded accent-[var(--ct-accent)]"
                />
                <span className="text-[12px] text-[var(--ct-muted-fg)]">Remember my choice</span>
              </label>
            </div>
            <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-[var(--ct-border)]">
              <Button variant="ghost" size="sm" onClick={() => setPermissionModalProviderId(null)}>Cancel</Button>
              <Button variant="accent" size="sm" onClick={handlePermissionConfirm}>Launch</Button>
            </div>
          </div>
        </>
      )}

      {folderPickerOpen && (
        <div className="fixed inset-0 z-[1020]">
          <RemoteFolderPicker
            title="Choose workspace folder"
            initialPath={selectedWorkspace?.cwd ?? workspaceCwd}
            onCancel={() => setFolderPickerOpen(false)}
            onSelect={handleSelectWorkspaceFolder}
          />
        </div>
      )}
    </div>,
    document.body,
  );
}
