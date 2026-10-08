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
  planSelection,
  statusLineSwitchGate,
  type PickerSelection,
  type StatusLineTabFacts,
} from "./statusLinePickerModel";

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
      modelCandidates={[tab.agent_provider_resolved_model, tab.agent_session_model]}
      currentEffort={effort?.actual ?? effort?.resolved ?? effort?.requested ?? undefined}
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
}: {
  tabId: string;
  descriptor: ProviderDescriptor | undefined;
  facts: StatusLineTabFacts;
  label: string;
  modelCandidates: Array<string | null | undefined>;
  currentEffort: string | undefined;
}) {
  const gate = statusLineSwitchGate(descriptor, facts);
  const currentModel = currentModelId(descriptor, modelCandidates);
  const [open, setOpen] = useState(false);
  const [applied, setApplied] = useState<PickerSelection | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => {
    setOpen(false);
    triggerRef.current?.focus({ preventScroll: true });
  }, []);

  if (!gate.interactive || !descriptor) {
    return (
      <span className="ct-status-identity ct-status-identity--static" title={gate.interactive ? undefined : gate.reason}>
        {label}
      </span>
    );
  }

  const appliedPending = applied !== null && !selectionReached(descriptor, applied, currentModel, currentEffort);
  return (
    <span className="ct-status-identity-wrap">
      <button
        ref={triggerRef}
        type="button"
        className="ct-status-identity"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={`Change model, currently ${label}`}
        title="Change model"
        data-testid="status-model-trigger"
        onClick={() => setOpen((value) => !value)}
      >
        <span className="ct-status-identity__text">{label}</span>
        <ChevronDown size={12} strokeWidth={2.25} aria-hidden className="ct-status-identity__chevron" />
      </button>
      {appliedPending && gate.timing !== "live" && (
        <span role="status" className="ct-status-identity__pending">{gate.note}</span>
      )}
      {open && (
        <ModelPickerPopover
          tabId={tabId}
          descriptor={descriptor}
          gate={gate}
          triggerRef={triggerRef}
          currentModel={currentModel}
          currentEffort={currentEffort}
          onApplied={(selection) => {
            setApplied(selection);
            close();
          }}
          onClose={close}
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
  return modelReached && (selection.effort === undefined || selection.effort === currentEffort);
}

function ModelPickerPopover({
  tabId,
  descriptor,
  gate,
  triggerRef,
  currentModel,
  currentEffort,
  onApplied,
  onClose,
}: {
  tabId: string;
  descriptor: ProviderDescriptor;
  gate: Extract<ReturnType<typeof statusLineSwitchGate>, { interactive: true }>;
  triggerRef: RefObject<HTMLButtonElement | null>;
  currentModel: string | undefined;
  currentEffort: string | undefined;
  onApplied: (selection: PickerSelection) => void;
  onClose: () => void;
}) {
  const titleId = useId();
  const popoverRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [staged, setStaged] = useState<PickerSelection | null>(
    currentModel ? { model: currentModel, effort: currentEffort } : null,
  );
  const [confirming, setConfirming] = useState<PickerSelection | null>(null);
  const [restartFallback, setRestartFallback] = useState(false);
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [anchor, setAnchor] = useState<{ left: number; bottom: number } | null>(null);
  const searchable = pickerNeedsSearch(descriptor);
  const groups = pickerModelGroups(descriptor, staged?.model, query);
  const efforts = effortChoices(descriptor, staged?.model);
  const stagedEffort = staged ? effortAfterModelChange(descriptor, staged.model, staged.effort) : undefined;

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

  async function apply(selection: PickerSelection, restart = false) {
    setBusy(true);
    setError(null);
    try {
      const api = getTabModelApi();
      if (restart) {
        if (!api.restartAndSwitchModel) throw new Error("Restart fallback is unavailable");
        await api.restartAndSwitchModel(tabId, selection.model, selection.effort);
      } else {
        await api.switchModel(tabId, selection.model, selection.effort);
      }
      onApplied(selection);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      setBusy(false);
    }
  }

  function choose(next: PickerSelection) {
    setRestartFallback(false);
    setStaged(next);
    const plan = planSelection(gate, { model: currentModel, effort: currentEffort }, next);
    setConfirming(plan.action === "confirm" ? plan.selection : null);
    if (plan.action === "apply") void apply(plan.selection);
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
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        onClose();
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
                  if (event.key !== "Enter" && event.key !== " ") return;
                  event.preventDefault();
                  if (!row.unavailable && !busy) chooseModel(row.id);
                }}
              >
                <span className="ct-model-picker__name">{row.label}</span>
                {row.badge && <span className="ct-model-picker__badge">{row.badge}</span>}
                {isCurrent && <Check size={13} strokeWidth={2.5} aria-label="Current model" className="ct-model-picker__check" />}
              </div>
              );
            })}
          </div>
        ))}
      </div>
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
            <button type="button" className={UI_BUTTON.primary} disabled={busy} onClick={() => void apply(confirming, restartFallback)}>
              Restart and switch
            </button>
          </div>
        </div>
      ) : (
        <p className="ct-model-picker__note">{gate.note}</p>
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
