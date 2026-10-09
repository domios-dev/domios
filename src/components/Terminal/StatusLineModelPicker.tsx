import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown, RotateCw } from "lucide-react";
import type { ProviderDescriptor, Tab } from "../../lib/types";
import { useAgentDescriptor } from "../../lib/useAgentDescriptor";
import { getModelCatalogueRefreshApi } from "../../lib/modelCatalogueRefresh";
import { getTabModelApi } from "../Sidebar/tabModelApi";
import { BusySpinner } from "../shared/BusyState";
import { Segmented } from "../shared/Segmented";
import { UI_BUTTON } from "../shared/design";
import {
  currentModelId,
  effortAfterModelChange,
  effortChoices,
  matchesModel,
  pickerAnchor,
  pickerModelGroups,
  pickerNeedsSearch,
  pickerHasPrimaryModels,
  planSelection,
  statusLineSwitchGate,
  type PickerSelection,
  type StatusLineTabFacts,
} from "./statusLinePickerModel";

import { readPickerShowAll, writePickerShowAll } from "./statusLinePickerPreference";

const PICKER_WIDTH = 300;
const OPTION_SELECTOR = '[role="option"]:not([aria-disabled="true"])';

/** The status-line identity for a tab, made switchable when its provider declares it. */
export function TabStatusLineModelPicker({ tab, label }: { tab: Tab; label: string }) {
  const descriptor = useAgentDescriptor(tab.agent_type);
  const effort = tab.reasoning_effort_identity;
  return (
    <StatusLineModelPicker
      tabId={tab.id}
      descriptor={descriptor}
      facts={{ hasSession: Boolean(tab.pty_id) || tab.backing?.kind === "chat_backend", protected: tab.protected === true }}
      label={label}
      modelCandidates={[tab.agent_provider_resolved_model, tab.agent_session_model, tab.agent_requested_model]}
      currentEffort={effort?.actual ?? effort?.resolved ?? effort?.requested ?? undefined}
      pendingSelection={tab.pending_model_selection ?? undefined}
    />
  );
}

export function StatusLineModelPicker({
  tabId,
  descriptor,
  facts,
  label,
  modelCandidates,
  currentEffort,
  pendingSelection,
}: {
  tabId: string;
  descriptor: ProviderDescriptor | undefined;
  facts: StatusLineTabFacts;
  label: string;
  modelCandidates: Array<string | null | undefined>;
  currentEffort: string | undefined;
  pendingSelection?: { model: string; effort: string | null };
}) {
  const gate = statusLineSwitchGate(descriptor, facts);
  const currentModel = currentModelId(descriptor, modelCandidates);
  const [open, setOpen] = useState(false);
  const [applied, setApplied] = useState<PickerSelection | null>(null);
  const [applying, setApplying] = useState(false);
  const [stagedAfterTurn, setStagedAfterTurn] = useState(false);
  const [failure, setFailure] = useState<{ message: string; selection: PickerSelection } | null>(null);
  const appliedRef = useRef<PickerSelection | null>(null);
  appliedRef.current = applied;
  const triggerRef = useRef<HTMLButtonElement>(null);
  const sawPendingRef = useRef(false);
  useEffect(() => {
    if (pendingSelection) {
      sawPendingRef.current = true;
    } else if (sawPendingRef.current) {
      sawPendingRef.current = false;
      setApplied(null);
      setStagedAfterTurn(false);
    }
  }, [pendingSelection]);
  useEffect(() => {
    if (descriptor && applied && selectionReached(descriptor, applied, currentModel, currentEffort)) {
      setApplied(null);
    }
  }, [descriptor, applied, currentModel, currentEffort]);
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
      if (restart) {
        if (!api.restartAndSwitchModel) throw new Error("Restart fallback is unavailable");
        const result = await api.restartAndSwitchModel(tabId, selection.model, selection.effort);
        setStagedAfterTurn(result?.staged === true);
      } else {
        const result = await api.switchModel(tabId, selection.model, selection.effort);
        setStagedAfterTurn(result?.staged === true);
      }
    } catch (error) {
      setApplied(previous);
      setFailure({ message: error instanceof Error ? error.message : String(error), selection });
      setOpen(true);
    } finally {
      setApplying(false);
    }
  }, [close, tabId]);

  const selection = pendingSelection ? { model: pendingSelection.model, effort: pendingSelection.effort ?? undefined }
    : applied ?? { model: currentModel ?? "Unknown model", effort: currentEffort };
  const modelName = descriptor?.available_models.find((model) => matchesModel(model, selection.model))?.display_name
    || selection.model;
  const afterTurn = Boolean(pendingSelection) || (stagedAfterTurn && Boolean(applied));
  const displayedLabel = [modelName, selection.effort, afterTurn ? "after this turn" : undefined].filter(Boolean).join(" · ");
  const harnessName = descriptor?.display_name ?? label.split(" · ")[0];

  if (!gate.interactive || !descriptor) {
    return (
      <span className="ct-status-identity-wrap">
        <span className="ct-status-harness">{harnessName}</span>
        <span className="ct-status-identity ct-status-identity--static" title={gate.interactive ? undefined : gate.reason}>
          {displayedLabel}
        </span>
      </span>
    );
  }

  return (
    <span className="ct-status-identity-wrap">
      <span className="ct-status-harness">{harnessName}</span>
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
        <span className="ct-status-identity__text">{displayedLabel}</span>
        <ChevronDown size={12} strokeWidth={2.25} aria-hidden className="ct-status-identity__chevron" />
      </button>
      {applying && <span className="ct-status-identity__pending" role="status">Applying…</span>}
      {open && (
        <ModelPickerPopover
          key={descriptor.id}
          descriptor={descriptor}
          gate={gate}
          triggerRef={triggerRef}
          currentModel={selection.model}
          currentEffort={selection.effort}
          busy={applying}
          failure={failure}
          onCommit={(selection, restart) => void commit(selection, restart)}
          onClose={() => {
            void getTabModelApi().cancelStagedModel?.(tabId).then(() => { setApplied(null); setStagedAfterTurn(false); }).catch((error: unknown) => {
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
): boolean {
  const model = descriptor.available_models.find((entry) => entry.id === selection.model);
  const modelReached = model ? matchesModel(model, currentModel) : selection.model === currentModel;
  return modelReached && selection.effort === currentEffort;
}

function ModelPickerPopover({
  descriptor,
  gate,
  triggerRef,
  currentModel,
  currentEffort,
  busy,
  failure,
  onCommit,
  onClose,
}: {
  descriptor: ProviderDescriptor;
  gate: Extract<ReturnType<typeof statusLineSwitchGate>, { interactive: true }>;
  triggerRef: RefObject<HTMLButtonElement | null>;
  currentModel: string | undefined;
  currentEffort: string | undefined;
  busy: boolean;
  failure: { message: string; selection: PickerSelection } | null;
  onCommit: (selection: PickerSelection, restart: boolean) => void;
  onClose: () => void;
}) {
  const titleId = useId();
  const popoverRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [showAll, setShowAll] = useState(() => readPickerShowAll(descriptor.id));
  const [staged, setStaged] = useState<PickerSelection | null>(
    failure?.selection ?? (currentModel ? { model: currentModel, effort: currentEffort } : null),
  );
  const [confirming, setConfirming] = useState<PickerSelection | null>(null);
  const [restartFallback, setRestartFallback] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(failure?.message ?? null);
  const [anchor, setAnchor] = useState<{ left: number; bottom: number } | null>(null);
  const searchable = pickerNeedsSearch(descriptor);
  const groups = pickerModelGroups(descriptor, staged?.model, query, showAll);
  const efforts = effortChoices(descriptor, staged?.model);
  const stagedEffort = staged ? effortAfterModelChange(descriptor, staged.model, staged.effort) : undefined;
  const selectionPlan = staged ? planSelection(gate, { model: currentModel, effort: currentEffort }, staged) : { action: "none" };

  useLayoutEffect(() => {
    const place = () => {
      const rect = triggerRef.current?.getBoundingClientRect();
      if (!rect) return;
      setAnchor(pickerAnchor(rect, { width: window.innerWidth, height: window.innerHeight }, PICKER_WIDTH));
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [triggerRef]);

  useLayoutEffect(() => {
    if (searchable) {
      searchRef.current?.focus({ preventScroll: true });
      return;
    }
    const selected = popoverRef.current?.querySelector<HTMLElement>('[role="option"][aria-selected="true"]');
    (selected ?? popoverRef.current?.querySelector<HTMLElement>(OPTION_SELECTOR))?.focus({ preventScroll: true });
  }, [searchable]);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (!target || popoverRef.current?.contains(target) || triggerRef.current?.contains(target)) return;
      onClose();
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [onClose, triggerRef]);

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
    const plan = planSelection(gate, { model: currentModel, effort: currentEffort }, staged);
    if (plan.action === "confirm") setConfirming(plan.selection);
    if (plan.action === "apply") apply(plan.selection);
  }

  function chooseModel(model: string) {
    choose({ model, effort: effortAfterModelChange(descriptor, model, staged?.effort ?? currentEffort) });
  }

  async function refresh() {
    setRefreshing(true);
    setError(null);
    try {
      if (!(await getModelCatalogueRefreshApi().refresh(descriptor.id))) {
        setError("The model list could not be refreshed. The current list stays in use.");
      }
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
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

  return createPortal(
    <div
      ref={popoverRef}
      role="dialog"
      aria-labelledby={titleId}
      data-testid="status-model-picker"
      className="ct-model-picker ct-float"
      style={{ width: PICKER_WIDTH, left: anchor?.left ?? 0, bottom: anchor?.bottom ?? 0, visibility: anchor ? undefined : "hidden" }}
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
        <span id={titleId} className="ct-model-picker__title">{descriptor.display_name}</span>
        <button
          type="button"
          className="ct-model-picker__refresh"
          aria-label="Refresh models"
          title="Refresh models"
          disabled={refreshing}
          onClick={() => void refresh()}
        >
          <BusySpinner icon={RotateCw} size={13} spinning={refreshing} />
        </button>
      </div>
      {searchable && (
        <input
          ref={searchRef}
          type="search"
          className="ct-model-picker__search"
          aria-label="Search models"
          placeholder="Search models"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== "ArrowDown") return;
            event.preventDefault();
            popoverRef.current?.querySelector<HTMLElement>(OPTION_SELECTOR)?.focus();
          }}
        />
      )}
      <div role="listbox" aria-label="Models" className="ct-model-picker__list" onKeyDown={onListKeyDown}>
        {groups.length === 0 && <p className="ct-model-picker__empty">No model matches “{query.trim()}”.</p>}
        {groups.map((group) => (
          <div key={group.label || "models"} role="group" aria-label={group.label || "Models"}>
            {group.label && <div className="ct-model-picker__group">{group.label}</div>}
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
                className="ct-model-picker__option"
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
      {pickerHasPrimaryModels(descriptor) && (
        <button
          type="button"
          className="ct-model-picker__toggle"
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
      {efforts.length > 0 && staged && stagedEffort && (
        <div className="ct-model-picker__effort">
          <span className="ct-model-picker__label">Reasoning</span>
          <Segmented
            aria-label="Reasoning"
            value={stagedEffort}
            options={efforts.map((effort) => ({ value: effort.id, label: effort.display_name }))}
            onChange={(effort) => { if (!busy) choose({ model: staged.model, effort }); }}
          />
        </div>
      )}
      {confirming ? (
        <div className="ct-model-picker__confirm" role="group" aria-label="Confirm restart">
          <p>Switching restarts this session on {stagedLabel} and resumes it.</p>
          <div className="ct-model-picker__actions">
            <button type="button" className={UI_BUTTON.ghost} onClick={() => { setConfirming(null); setStaged(currentModel ? { model: currentModel, effort: currentEffort } : null); }}>
              Keep current
            </button>
            <button type="button" className={UI_BUTTON.primary} disabled={busy} onClick={() => apply(confirming, restartFallback)}>
              Restart and switch
            </button>
          </div>
        </div>
      ) : (
        <div className="ct-model-picker__footer">
          <p className="ct-model-picker__note">{gate.note}</p>
          <div className="ct-model-picker__actions">
            <button type="button" className={UI_BUTTON.ghost} onClick={onClose}>Cancel</button>
            <button type="button" className={UI_BUTTON.primary} disabled={busy || selectionPlan.action === "none"} onClick={commit}>Apply</button>
          </div>
        </div>
      )}
      {error && <p role="alert" className="ct-model-picker__error">{error}</p>}
      {error && gate.timing === "live_command" && !confirming && staged && (
        <button type="button" className={UI_BUTTON.ghost} disabled={busy}
          onClick={() => { setRestartFallback(true); setConfirming(staged); }}>
          Restart and resume instead
        </button>
      )}
    </div>,
    document.body,
  );
}
