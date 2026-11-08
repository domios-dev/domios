import { createNoticePublisher } from "../../lib/notices";
import type { RuntimeAskScope, RuntimeServiceTierSelection as ServiceTierSelection } from "../../../packages/plugin-sdk/src/agentRuntime";
import { confirmedTabServiceTier, serviceTierLabel, tabServiceTierAvailability, tabMetaParts } from "../../lib/tabMetaLabel";
import { tierAfterModelChange } from "./serviceTierPickerModel";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useCallback, useContext, useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { Check, ChevronDown, RotateCw, Search, X } from "lucide-react";
import { MultiplexerActionsContext } from "../../store/multiplexer/context";
import type { ProviderDescriptor, Tab } from "../../lib/types";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { getModelCatalogueRefreshApi, refreshFailureToast } from "../../lib/modelCatalogueRefresh";
import { getTabModelApi } from "../Sidebar/tabModelApi";
import { modelSwitchFailure, runtimeBusy } from "../../lib/modelSwitchFailure";
import { BusySpinner } from "../shared/BusyState";
import { UI_BUTTON } from "../shared/design";
import { AgentIcon } from "../AgentIcon";
import { ProviderGlyph } from "../ProviderGlyph";
import { runtimeAgentLabel } from "../../lib/tabProvider";
import { selectRuntimeControl, setHostApprovals } from "../../lib/runtimeDeclaredControls";
import { declaredAskScope, hostApprovalsChoice, hostFullAccessOverride, pickerSections, pickerDescriptor, recoveringModeLabel, sessionModeNames, type HostApprovalsValue, type PickerSection } from "../../lib/pickerSettings";
import { PickerSettingSection } from "./PickerSettingSection";
import {
  currentModelId,
  effortAfterModelChange,
  matchesModel,
  pickerModelGroups,
  pickerNeedsSearch,
  pickerHasPrimaryModels,
  planSelection,
  pickerSelection,
  statusLineSwitchGate,
  statusLineModelName,
  runtimeAgentTooltip,
  pickerHarnessName,
  type PickerSelection,
  type StatusLineTabFacts,
} from "./statusLinePickerModel";
import { AnchoredPopover } from "../shared/AnchoredPopover";

import { readPickerShowAll, writePickerShowAll } from "./statusLinePickerPreference";

const PICKER_WIDTH = 300;
const OPTION_SELECTOR = '[role="option"]:not([aria-disabled="true"])';
const FOOTER_BUTTON = {
  ghost: `${UI_BUTTON.ghost} ${UI_BUTTON.compact}`,
  primary: `${UI_BUTTON.primary} ${UI_BUTTON.compact}`,
} as const;

/** The status-line identity for a tab, made switchable when its provider declares it. */
export function TabStatusLineModelPicker({ tab, label }: { tab: Tab; label: string }) {
  const descriptor = useAgentDescriptor(tab.agent_type);
  const effort = tab.reasoning_effort_identity;
  return (
    <StatusLineModelPicker
      tab={tab}
      tabId={tab.id}
      descriptor={descriptor}
      runtimeControls={tab.agent_runtime_state?.controls}
      runtimeIdentity={tab.agent_runtime_state}
      facts={{ hasSession: Boolean(tab.pty_id) || tab.backing?.kind === "chat_backend" }}
      label={label}
      modelCandidates={[statusLineModelName(tab)]}
      currentServiceTier={confirmedTabServiceTier(tab, descriptor)}
      serviceTierAvailability={tabServiceTierAvailability(tab)}
      currentRuntimeMode={tab.structured_runtime_mode ?? undefined}
      currentEffort={effort?.actual ?? effort?.resolved ?? effort?.requested ?? undefined}
      pendingSelection={tab.pending_model_selection ?? (tab.pending_model_switch_observation ? {
        model: tab.pending_model_switch_observation.launched_model,
        effort: effort?.resolved ?? null,
      } : undefined)}
    />
  );
}

export function StatusLineModelPicker({
  tab,
  tabId,
  descriptor,
  facts,
  label,
  modelCandidates,
  currentEffort,
  currentServiceTier,
  currentRuntimeMode,
  serviceTierAvailability,
  pendingSelection,
  runtimeControls,
  runtimeIdentity,
}: {
  tab?: Tab;
  tabId: string;
  runtimeControls?: import("@codeterm/plugin-sdk").RuntimeSettings;
  runtimeIdentity?: {agentName?:string;binaryVersion?:string} | null;
  descriptor: ProviderDescriptor | undefined;
  facts: StatusLineTabFacts;
  label: string;
  modelCandidates: Array<string | null | undefined>;
  currentEffort: string | undefined;
  currentServiceTier?: ServiceTierSelection;
  currentRuntimeMode?: string;
  serviceTierAvailability?: Record<string,readonly import("../../../packages/plugin-sdk/src/agentRuntime").RuntimeServiceTier[]>;
  pendingSelection?: { model: string; effort: string | null; confirmation_pending?: boolean };
}) {
  const modeNames = useRef(new Map<string, string>());
  for (const [id, name] of sessionModeNames(runtimeControls)) modeNames.current.set(id, name);
  const unavailableLabel = recoveringModeLabel(tab?.agent_runtime_state?.state, tab?.agent_runtime_config, modeNames.current) ?? "Default";
  const hostApprovals = tab ? hostApprovalsChoice(descriptor, tab.agent_runtime_config, runtimeControls) : undefined;
  const hostAskScope = tab ? declaredAskScope(descriptor, tab.agent_runtime_config) : undefined;
  const hostFullAccess = tab ? hostFullAccessOverride(descriptor, tab.agent_runtime_config, runtimeControls) : false;
  const settings = pickerSections({ descriptor, controls: runtimeControls, model: currentModelId(descriptor, modelCandidates), effort: currentEffort, hostApprovals, hostAskScope, hostFullAccess });
  const modelSection = settings.find(section => section.category === "model");
  const effortSection = settings.find(section => section.category === "effort");
  const declaredModels = modelSection?.applyTarget.kind === "config" || Boolean(descriptor?.agent_runtime && !descriptor.available_models.length);
  if (descriptor) descriptor = pickerDescriptor(descriptor, settings);
  if (declaredModels) currentEffort = effortSection?.current;
  const gate = statusLineSwitchGate(descriptor, facts);
  const currentModel = declaredModels ? modelSection?.current : currentModelId(descriptor, modelCandidates);
  const [open, setOpen] = useState(false);
  const [applied, setApplied] = useState<PickerSelection | null>(null);
  const [applying, setApplying] = useState(false);
  const [failure, setFailure] = useState<{ message: string; selection: PickerSelection } | null>(null);
  const appliedRef = useRef<PickerSelection | null>(null);
  appliedRef.current = applied;
  const triggerRef = useRef<HTMLButtonElement>(null);
  const actions = useContext(MultiplexerActionsContext);
  const commitRef = useRef<(selection: PickerSelection, restart: boolean) => Promise<void>>(async () => undefined);
  useEffect(() => {
    if (descriptor && applied && selectionReached(descriptor, applied, currentModel, currentEffort, currentServiceTier, currentRuntimeMode)) {
      setApplied(null);
    }
  }, [descriptor, applied, currentModel, currentEffort, currentServiceTier, currentRuntimeMode]);
  const close = useCallback(() => {
    setOpen(false);
    triggerRef.current?.focus({ preventScroll: true });
  }, []);
  const commit = useCallback(async (selection: PickerSelection, restart: boolean) => {
    const previous = appliedRef.current;
    setApplied(selection);
    setApplying(true);
    setFailure(null);
    close();
    try {
      const api = getTabModelApi();
      if (declaredModels) {
        const target = { threadId: tab?.agent_session_id, bindingNonce: tab?.session_binding_nonce, epoch: tab?.agent_runtime_state?.epoch };
        if (modelSection?.applyTarget.kind === "config" && selection.model !== modelSection.current) await selectRuntimeControl(tabId, target, modelSection.applyTarget.id, selection.model);
        if (effortSection?.applyTarget.kind === "config" && selection.effort && selection.effort !== effortSection.current) await selectRuntimeControl(tabId, target, effortSection.applyTarget.id, selection.effort);
      } else if (restart) {
        if (!api.restartAndSwitchModel) throw new Error("Restart fallback is unavailable");
        const result = await api.restartAndSwitchModel(tabId, selection.model, selection.effort, selection.runtimeMode);
        if (result?.staged) setApplied(null);
      } else {
        const result = selection.runtimeMode === undefined && selection.serviceTier === undefined
          ? await api.switchModel(tabId, selection.model, selection.effort)
          : await api.switchModel(tabId, selection.model, selection.effort, selection.runtimeMode, selection.serviceTier);
        if (result?.staged) setApplied(null);
      }
    } catch (error) {
      setApplied(previous);
      const failure = modelSwitchFailure(error, descriptor?.transport === "chat_backend");
      const busy = runtimeBusy(error);
      actions && createNoticePublisher(actions)({ kind: "toast", id: `model-switch:${tabId}`,
        tabId: tabId,
        workspaceId: null,
        title: busy ? "Model not changed yet" : "Model switch did not apply",
        detail: failure.message,
        severity: busy ? "Info" : "Warning",
        lifetimeMs: busy ? 6000 : 15000,
        actions: restart || !failure.restartAvailable ? undefined : [{ label: "Restart and resume", run: () => void commitRef.current(selection, true) }] });
    } finally {
      setApplying(false);
    }
  }, [actions, close, descriptor?.transport, tabId, tab, declaredModels, modelSection, effortSection]);
  commitRef.current = commit;

  const selection = pendingSelection ? { model: pendingSelection.model, effort: pendingSelection.effort ?? undefined }
    : applied ?? { model: currentModel ?? "Unknown model", effort: currentEffort, ...(descriptor?.agent_runtime?.serviceTiers ? { serviceTier: currentServiceTier } : {}), ...(descriptor?.agent_runtime?.runtimeModes?.length ? { runtimeMode: currentRuntimeMode } : {}) };
  const modelName = descriptor?.available_models.find((model) => matchesModel(model, selection.model))?.display_name
    || selection.model;
  const afterTurn = Boolean(pendingSelection);
  const confirmedModelName = declaredModels ? descriptor?.available_models.find(model => model.id === currentModel)?.display_name ?? currentModel : modelName;
  const tierLabel = serviceTierLabel(descriptor, currentServiceTier);
  const selectionStatus = pendingSelection?.confirmation_pending ? "Switch sent; confirms when this turn ends" : afterTurn ? "after this turn" : undefined;
  const displayedLabel = declaredModels
    ? tabMetaParts(confirmedModelName || unavailableLabel, tierLabel, selection.effort, selectionStatus).join(" · ")
    : tabMetaParts(confirmedModelName, tierLabel, selection.effort, selectionStatus).join(" · ");
  // The logo names the agent; the agent's reported name and version live only in the tooltip, never in the label.
  const providerName = descriptor?.display_name ?? label.split(" · ")[0];
  const harnessTitle = (tab ? runtimeAgentLabel(tab) : null) ?? runtimeAgentTooltip(runtimeIdentity) ?? providerName;
  const glyph = descriptor ? (
    <span aria-hidden="true" className="ct-status-identity__glyph" data-testid="status-harness-glyph" title={harnessTitle}>
      {tab ? <AgentIcon providerId={descriptor.id} tab={tab} title={harnessTitle} />
        : <ProviderGlyph iconSvg={descriptor.icon_svg} iconColor={descriptor.icon_color} iconHint={descriptor.icon_hint} />}
    </span>
  ) : null;

  if (declaredModels && !runtimeControls?.configOptions && !settings.length) return (
    <span className="ct-status-identity ct-status-identity--static" title={harnessTitle || "Model controls are unavailable"}>
      {glyph}{providerName || "Agent"}
    </span>
  );

  // Nothing to choose: the agent's logo and a quiet Default, with no menu.
  if (declaredModels && !settings.some(section => section.options.length)) return (
    <span className="ct-status-identity ct-status-identity--static" data-testid="status-model-default"
      title={`${harnessTitle ? `${harnessTitle}. ` : ""}This agent does not offer model choices`}>
      {glyph}<span className="ct-status-identity__text">{[unavailableLabel, selectionStatus].filter(Boolean).join(" · ")}</span>
    </span>
  );

  if (!gate.interactive || !descriptor) {
    return (
      <span className="ct-status-identity-wrap">
        <span className="ct-status-identity ct-status-identity--static" title={gate.interactive ? harnessTitle : gate.reason}>
          {glyph}
          <span className="ct-status-identity__text">{displayedLabel}</span>
        </span>
      </span>
    );
  }

  return (
    <span className="ct-status-identity-wrap">
      <button
        ref={triggerRef}
        type="button"
        className="ct-status-identity"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={`Change model, currently ${displayedLabel}`}
        title="Change model"
        data-testid="status-model-trigger"
        onClick={() => setOpen((value) => !value)}
      >
        {glyph}
        <span className="ct-status-identity__text">{displayedLabel}</span>
        <ChevronDown size={12} strokeWidth={2.25} aria-hidden className="ct-status-identity__chevron" />
      </button>
      {applying && <span className="ct-status-identity__pending" role="status">Applying…</span>}
      {open && (
        <ModelPickerPopover
          harnessName={pickerHarnessName(descriptor, runtimeIdentity)}
          key={descriptor.id}
          descriptor={descriptor}
          gate={gate}
          declaredModels={declaredModels}
          runtimeControls={runtimeControls}
          hostApprovals={hostApprovals}
          hostAskScope={hostAskScope}
          hostFullAccess={hostFullAccess}
          onRuntimeControl={async (section, value) => {
            setApplying(true);
            try {
              if (section.applyTarget.kind === "host_approvals") await setHostApprovals(tabId, value === "allow_all");
              else await selectRuntimeControl(tabId, { threadId: tab?.agent_session_id, bindingNonce: tab?.session_binding_nonce, epoch: tab?.agent_runtime_state?.epoch }, section.applyTarget.kind === "config" ? section.applyTarget.id : "", value, section.applyTarget.kind === "session");
            } finally { setApplying(false); }
          }}
          triggerRef={triggerRef}
          currentModel={declaredModels ? currentModel : selection.model}
          currentServiceTier={selection.serviceTier}
          currentRuntimeMode={selection.runtimeMode}
          serviceTierAvailability={serviceTierAvailability}
          currentEffort={selection.effort}
          busy={applying}
          failure={failure}
          onCommit={(selection, restart) => void commit(selection, restart)}
          onClose={() => {
            if (!pendingSelection?.confirmation_pending) void getTabModelApi().cancelStagedModel?.(tabId).then(() => { setApplied(null); }).catch((error: unknown) => {
              setFailure({ message: String(error), selection });
            });
            setFailure(null); close();
          }}
        />
      )}
    </span>
  );
}

function selectionReached(
  descriptor: ProviderDescriptor,
  selection: PickerSelection,
  currentModel: string | undefined,
  currentEffort: string | undefined,
  currentServiceTier: ServiceTierSelection | undefined,
  currentRuntimeMode: string | undefined,
): boolean {
  const model = descriptor.available_models.find((entry) => entry.id === selection.model);
  const modelReached = model ? matchesModel(model, currentModel) : selection.model === currentModel;
  return modelReached && selection.effort === currentEffort && selection.serviceTier?.id === currentServiceTier?.id && selection.runtimeMode === currentRuntimeMode;
}

export function ModelPickerPopover({
  harnessName,
  descriptor,
  gate,
  triggerRef,
  currentModel,
  currentEffort,
  currentServiceTier,
  currentRuntimeMode,
  serviceTierAvailability,
  showReasoning = true,
  declaredModels = false,
  runtimeControls,
  hostApprovals,
  hostAskScope,
  hostFullAccess,
  onRuntimeControl,
  busy,
  failure,
  onCommit,
  onClose,
}: {
  harnessName?: string;
  descriptor: ProviderDescriptor;
  gate: Extract<ReturnType<typeof statusLineSwitchGate>, { interactive: true }>;
  triggerRef: RefObject<HTMLButtonElement | null>;
  currentModel: string | undefined;
  currentEffort: string | undefined;
  currentServiceTier?: ServiceTierSelection;
  currentRuntimeMode?: string;
  serviceTierAvailability?: Record<string,readonly import("../../../packages/plugin-sdk/src/agentRuntime").RuntimeServiceTier[]>;
  showReasoning?: boolean;
  declaredModels?: boolean;
  runtimeControls?: import("@codeterm/plugin-sdk").RuntimeSettings;
  hostApprovals?: HostApprovalsValue;
  hostAskScope?: RuntimeAskScope;
  hostFullAccess?: boolean;
  onRuntimeControl?: (section: PickerSection, value: string) => Promise<void>;
  busy: boolean;
  failure: { message: string; selection: PickerSelection } | null;
  onCommit: (selection: PickerSelection, restart: boolean) => void;
  onClose: () => void;
}) {
  const titleId = useId();
  const actions = useContext(MultiplexerActionsContext);
  const popoverRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [searchOpen, setSearchOpen] = useState(false);
  const [showAll, setShowAll] = useState(() => readPickerShowAll(descriptor.id));
  const [staged, setStaged] = useState<PickerSelection | null>(
    failure?.selection ?? (currentModel ? pickerSelection(descriptor, currentModel, currentEffort, currentServiceTier, currentRuntimeMode) : null),
  );
  const [confirming, setConfirming] = useState<PickerSelection | null>(null);
  const [restartFallback, setRestartFallback] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(failure?.message ?? null);
  const [notice, setNotice] = useState<string | null>(null);
  const searchable = pickerNeedsSearch(descriptor);
  const groups = pickerModelGroups(descriptor, staged?.model, query, showAll, declaredModels ? "" : currentModel);
  const sections = pickerSections({ descriptor, controls: runtimeControls, model: staged?.model ?? currentModel, effort: staged?.effort ?? currentEffort, runtimeMode: staged?.runtimeMode ?? currentRuntimeMode, serviceTier: staged?.serviceTier ?? currentServiceTier, serviceTierAvailability, hostApprovals, hostAskScope, hostFullAccess });
  const effortSetting = sections.find(section => section.category === "effort");
  const extraSettings = sections.filter(section => section.category !== "model" && section.category !== "effort");
  const stagedEffort = staged ? effortAfterModelChange(descriptor, staged.model, staged.effort) : undefined;
  const selectionPlan = staged ? planSelection(gate, { model: currentModel, effort: currentEffort, ...(descriptor?.agent_runtime?.serviceTiers ? { serviceTier: currentServiceTier } : {}), ...(descriptor.agent_runtime?.runtimeModes?.length ? { runtimeMode: currentRuntimeMode } : {}) }, staged) : { action: "none" };

  // Picking a model is the intent; search is opt-in, so the list takes focus and the field only when opened.
  useLayoutEffect(() => {
    if (searchOpen) {
      searchRef.current?.focus({ preventScroll: true });
      return;
    }
    const selected = popoverRef.current?.querySelector<HTMLElement>('[role="option"][aria-selected="true"]');
    (selected ?? popoverRef.current?.querySelector<HTMLElement>(OPTION_SELECTOR))?.focus({ preventScroll: true });
  }, [searchOpen]);

  function apply(selection: PickerSelection, restart = false) {
    if (busy) return;
    onCommit(selection, restart);
  }

  function choose(next: PickerSelection) {
    setRestartFallback(false);
    setStaged(next);
    setConfirming(null);
    setError(null);
  }

  function commit() {
    if (busy || !staged) return;
    const plan = planSelection(gate, { model: currentModel, effort: currentEffort, ...(descriptor.agent_runtime?.serviceTiers ? { serviceTier: currentServiceTier } : {}), ...(descriptor.agent_runtime?.runtimeModes?.length ? { runtimeMode: currentRuntimeMode } : {}) }, staged);
    if (plan.action === "confirm") setConfirming(plan.selection);
    if (plan.action === "apply") apply(plan.selection);
  }

  function chooseModel(model: string) {
    choose({ ...staged, model, serviceTier:tierAfterModelChange(staged?.serviceTier,serviceTierAvailability?.[model]), effort: effortAfterModelChange(descriptor, model, staged?.effort ?? currentEffort) });
    closeSearch();
  }

  // Searching swaps the settings for results inside the same box, so the picker keeps the height it opened with.
  const [frozenHeight, setFrozenHeight] = useState<number | null>(null);
  function openSearch() {
    setFrozenHeight(popoverRef.current?.getBoundingClientRect().height ?? null);
    setSearchOpen(true);
  }
  function closeSearch() {
    setSearchOpen(false);
    setQuery("");
    setFrozenHeight(null);
  }

  async function refresh() {
    setRefreshing(true);
    const raise = (reason: string | undefined) => actions && createNoticePublisher(actions)(refreshFailureToast(descriptor.id, reason, (text) => navigator.clipboard?.writeText(text)));
    try {
      const outcome = await getModelCatalogueRefreshApi().refresh(descriptor.id);
      if (!outcome.refreshed) raise(outcome.reason);
    } catch (failure) {
      raise(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setRefreshing(false);
    }
  }

  function onListKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const options = Array.from(popoverRef.current?.querySelectorAll<HTMLElement>(OPTION_SELECTOR) ?? []);
    if (options.length === 0) return;
    const index = options.indexOf(document.activeElement as HTMLElement);
    const target = event.key === "ArrowDown" ? options[Math.min(options.length - 1, index + 1)]
      : event.key === "ArrowUp" ? options[Math.max(0, index - 1)]
        : event.key === "Home" ? options[0]
          : event.key === "End" ? options[options.length - 1] : null;
    if (!target) return;
    event.preventDefault();
    target.focus();
  }

  const stagedLabel = descriptor.available_models.find((model) => model.id === confirming?.model)?.display_name
    ?? confirming?.model;

  return (
    <AnchoredPopover open anchor={triggerRef} placement="top-start" width={PICKER_WIDTH} onClose={onClose}>
    <div
      ref={popoverRef}
      role="dialog"
      aria-labelledby={titleId}
      data-testid="status-model-picker"
      className="ct-menu ct-model-picker"
      style={{ width: PICKER_WIDTH, maxHeight: "min(440px, calc(100dvh - 24px))", maxWidth: "100%", overflowY: "auto", ...(frozenHeight ? { height: frozenHeight } : {}) }}
      onKeyDown={(event) => {
        if (event.code !== "Escape" && event.code !== "Enter" && event.code !== "NumpadEnter") return;
        if (event.code !== "Escape" && (event.target as HTMLElement).closest("button")) return;
        event.preventDefault();
        event.stopPropagation();
        if (event.code === "Escape") onClose();
        else if (confirming) void apply(confirming, restartFallback);
        else commit();
      }}
    >
      <div className="ct-model-picker__head">
        <span id={titleId} className="ct-model-picker__title">{harnessName ?? descriptor.display_name}</span>
        {searchable && !searchOpen && (
          <button
            type="button"
            className="ct-model-picker__refresh"
            aria-label="Search models"
            title="Search models"
            onClick={openSearch}
          >
            <Search size={13} aria-hidden />
          </button>
        )}
        {!declaredModels && <button
          type="button"
          className="ct-model-picker__refresh"
          aria-label="Refresh models"
          title="Refresh models"
          disabled={refreshing}
          onClick={() => void refresh()}
        >
          <BusySpinner icon={RotateCw} size={13} spinning={refreshing} />
        </button>}
      </div>
      {searchable && searchOpen && (
        <div className="relative">
          <input
            ref={searchRef}
            type="text"
            className="ct-model-picker__search"
            style={{ paddingRight: 30 }}
            aria-label="Search models"
            placeholder="Search models"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                closeSearch();
                return;
              }
              if (event.key !== "ArrowDown") return;
              event.preventDefault();
              popoverRef.current?.querySelector<HTMLElement>(OPTION_SELECTOR)?.focus();
            }}
          />
          <button
            type="button"
            className="ct-model-picker__refresh"
            aria-label="Close search"
            title="Close search"
            style={{ position: "absolute", right: 4, top: "50%", transform: "translateY(-50%)" }}
            onClick={closeSearch}
          >
            <X size={13} aria-hidden />
          </button>
        </div>
      )}
      <div role="listbox" aria-label="Models" className="ct-model-picker__list" onKeyDown={onListKeyDown}>
        {groups.length === 0 && <p className="ct-model-picker__empty">No model matches “{query.trim()}”.</p>}
        {groups.map((group) => (
          <div key={group.label || "models"} role="group" aria-label={group.label || "Models"}>
            {group.label && <div className="ct-menu-caption">{group.label}</div>}
            {group.rows.map((row) => {
              const isCurrent = matchesModel({ id: row.id, display_name: row.label }, currentModel);
              return (
              <div
                key={row.id}
                role="option"
                tabIndex={-1}
                aria-selected={row.current}
                aria-disabled={row.unavailable || busy || undefined}
                data-current={isCurrent || undefined}
                className="ct-menu-row ct-model-picker__option"
                title={row.unavailable ? "Not available on this account" : row.id}
                onClick={() => { if (!row.unavailable && !busy) chooseModel(row.id); }}
                onKeyDown={(event) => {
                  if (event.code !== "Space") return;
                  event.preventDefault();
                  if (!row.unavailable && !busy) chooseModel(row.id);
                }}
              >
                <span className="ct-model-picker__name">{row.label}</span>
                {row.badge && <span className="ct-model-picker__badge">{row.badge}</span>}
                {isCurrent && <span className="ct-model-picker__current">current</span>}
                {row.current && <Check size={13} strokeWidth={2.5} aria-label="Selected model" className="ct-model-picker__check" />}
              </div>
              );
            })}
          </div>
        ))}
      </div>
      {!searchOpen && pickerHasPrimaryModels(descriptor) && (
        <button
          type="button"
          className="ct-menu-row ct-model-picker__toggle"
          aria-pressed={showAll}
          onClick={() => {
            const next = !showAll;
            setShowAll(next);
            writePickerShowAll(descriptor.id, next);
          }}
        >
          {showAll ? "Show primary models" : "Show all models"}
        </button>
      )}
      <div role="separator" className="ct-menu-separator" />
      {!searchOpen && ((showReasoning && effortSetting) || extraSettings.length > 0) && (
        <div className="ct-model-picker__settings">
          {showReasoning && effortSetting && stagedEffort && <PickerSettingSection section={{ ...effortSetting, current: stagedEffort }} busy={busy}
            onChange={effort => { if (staged && !busy) choose({ ...staged, effort }); }} />}
          {extraSettings.length > 0 && <div className="ct-model-picker__extras">
            {extraSettings.map(section => <PickerSettingSection key={`${section.applyTarget.kind}:${section.id}`} section={section} busy={busy}
              onChange={value => {
                if (busy) return;
                if (section.applyTarget.kind === "mode") { if (staged) choose({ ...staged, runtimeMode: value }); }
                else if (section.applyTarget.kind === "speed") { if (staged) choose({ ...staged, serviceTier: { id: JSON.parse(value) as string | null } }); }
                else {
                  setNotice(null);
                  void onRuntimeControl?.(section, value).catch((failure: unknown) => {
                    if (runtimeBusy(failure)) setNotice(modelSwitchFailure(failure, true).message);
                    else setError("The agent refused this control value.");
                  });
                }
              }} />)}
          </div>}
        </div>
      )}
      {confirming ? (
        <div className="ct-model-picker__confirm" role="group" aria-label="Confirm restart">
          <p>Switching restarts this session on {stagedLabel} and resumes it.</p>
          <div className="ct-model-picker__actions">
            <button type="button" className={FOOTER_BUTTON.ghost} onClick={() => { setConfirming(null); setStaged(currentModel ? pickerSelection(descriptor, currentModel, currentEffort, currentServiceTier, currentRuntimeMode) : null); }}>
              Keep current
            </button>
            <button type="button" className={FOOTER_BUTTON.primary} disabled={busy} onClick={() => apply(confirming, restartFallback)}>
              Restart and switch
            </button>
          </div>
        </div>
      ) : (
        <div className="ct-model-picker__footer">
          {gate.timing !== "next_turn" && <p className="ct-model-picker__note">{gate.note}</p>}
          <div className="ct-model-picker__actions">
            <button type="button" className={FOOTER_BUTTON.ghost} onClick={onClose}>Cancel</button>
            <button type="button" className={FOOTER_BUTTON.primary} disabled={busy || selectionPlan.action === "none"} onClick={commit}>Apply</button>
          </div>
        </div>
      )}
      {notice && <p className="ct-model-picker__note" role="status">{notice}</p>}
      {error && (
        <div className="ct-model-picker__recovery">
          <InlineErrorNotice error={error} operation="changing the agent model" blocking />
          {gate.timing === "live_command" && !confirming && staged && (
            <button type="button" className={FOOTER_BUTTON.ghost} disabled={busy}
              onClick={() => { setRestartFallback(true); setConfirming(staged); }}>
              Restart and resume instead
            </button>
          )}
        </div>
      )}
    </div>
    </AnchoredPopover>
  );
}
