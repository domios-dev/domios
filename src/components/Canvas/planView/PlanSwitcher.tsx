import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { KebabGlyph } from "../../shared/RowMenuButton";

const SEGMENTED_MAX = 3;

export interface PlanSwitcherOption {
  id: string;
  title: string;
  archived?: boolean;
  /** Shown as "Open plan document" only when set. */
  docUrl?: string | null;
  /** In-flight task count for this plan (in_progress / in_review). */
  inflightCount?: number;
  /** Tooltip listing active task titles and executors. */
  inflightTooltip?: string | null;
}

export type PlanKebabAction =
  | { kind: "rename"; planId: string }
  | { kind: "archive"; planId: string }
  | { kind: "unarchive"; planId: string };

export interface PlanSwitcherProps {
  /** Pre-ordered by live activity freshness (freshest first). May include archived plans. */
  plans: readonly PlanSwitcherOption[];
  selectedId: string | null;
  onSelect: (planId: string) => void;
  onAction?: (action: PlanKebabAction) => void;
  label?: string;
}

function KebabMenu({
  plan,
  onAction,
  onClose,
  triggerEl,
}: {
  plan: PlanSwitcherOption;
  onAction: (a: PlanKebabAction) => void;
  onClose: () => void;
  triggerEl: HTMLButtonElement | null;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus();
  }, []);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node) && !triggerEl?.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [onClose, triggerEl]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
      triggerEl?.focus();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = Array.from(
      ref.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [],
    );
    const cur = items.indexOf(document.activeElement as HTMLButtonElement);
    const next =
      e.key === "ArrowDown"
        ? (cur + 1) % items.length
        : (cur - 1 + items.length) % items.length;
    items[next]?.focus();
  };

  const act = (action: PlanKebabAction) => {
    onAction(action);
    onClose();
  };

  const copyId = async () => {
    try {
      await navigator.clipboard.writeText(plan.id);
    } catch {
      /* clipboard unavailable in some environments */
    }
    onClose();
  };

  const doArchive = () => {
    act({ kind: "archive", planId: plan.id });
  };

  return (
    <div
      ref={ref}
      role="menu"
      data-testid="plan-switcher-menu"
      className="ct-menu plan-switcher-menu"
      onKeyDown={onKeyDown}
    >
      <button
        type="button"
        role="menuitem"
        className="ct-menu-row"
        onClick={() => act({ kind: "rename", planId: plan.id })}
      >
        Rename
      </button>
      <button
        type="button"
        role="menuitem"
        className="ct-menu-row"
        onClick={copyId}
      >
        Copy plan ID
      </button>
      {plan.docUrl && (
        <button
          type="button"
          role="menuitem"
          className="ct-menu-row"
          onClick={() => {
            window.open(plan.docUrl!, "_blank", "noreferrer");
            onClose();
          }}
        >
          Open plan document
        </button>
      )}
      {plan.archived ? (
        <button
          type="button"
          role="menuitem"
          className="ct-menu-row"
          onClick={() => act({ kind: "unarchive", planId: plan.id })}
        >
          Unarchive
        </button>
      ) : (
        <button
          type="button"
          role="menuitem"
          className="ct-menu-row"
          data-danger="true"
          onClick={doArchive}
        >
          Archive…
        </button>
      )}
    </div>
  );
}

function KebabTrigger({
  plan,
  isOpen,
  onToggle,
  refCallback,
}: {
  plan: PlanSwitcherOption;
  isOpen: boolean;
  onToggle: (e: React.MouseEvent) => void;
  refCallback: (el: HTMLButtonElement | null) => void;
}) {
  return (
    <button
      type="button"
      ref={refCallback}
      data-testid="plan-switcher-kebab"
      data-plan-id={plan.id}
      className="plan-switcher-kebab"
      title={`Actions for ${plan.title}`}
      aria-haspopup="menu"
      aria-expanded={isOpen}
      onClick={onToggle}
    >
      <KebabGlyph />
    </button>
  );
}

function inflightBadge(plan: PlanSwitcherOption) {
  const count = plan.inflightCount ?? 0;
  if (count <= 0) return null;
  return (
    <span
      data-testid="plan-switcher-badge"
      data-plan-id={plan.id}
      className="plan-switcher-inflight-badge"
      title={plan.inflightTooltip ?? `${count} in flight`}
      aria-label={`${count} in flight`}
    >
      {count}
    </span>
  );
}

function SegmentedSwitcher({
  activePlans,
  archivedPlans,
  selectedId,
  onSelect,
  onAction,
}: {
  activePlans: readonly PlanSwitcherOption[];
  archivedPlans: readonly PlanSwitcherOption[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onAction: (a: PlanKebabAction) => void;
}) {
  const [openMenu, setOpenMenu] = useState<{ id: string; el: HTMLButtonElement } | null>(null);
  const kebabEls = useRef<Map<string, HTMLButtonElement>>(new Map());

  const toggleMenu = useCallback((plan: PlanSwitcherOption, e: React.MouseEvent) => {
    e.stopPropagation();
    const el = kebabEls.current.get(plan.id);
    if (!el) return;
    setOpenMenu((prev) => (prev?.id === plan.id ? null : { id: plan.id, el }));
  }, []);

  const makePill = (plan: PlanSwitcherOption) => {
    const selected = plan.id === selectedId;
    const menuOpen = openMenu?.id === plan.id;
    const tip = plan.inflightTooltip ? `${plan.title}\n${plan.inflightTooltip}` : plan.title;

    return (
      <div
        key={plan.id}
        className="plan-switcher-pill"
        role="group"
        aria-label={plan.title}
      >
        <button
          type="button"
          data-testid="plan-switcher-option"
          data-plan-id={plan.id}
          data-active={selected ? "true" : "false"}
          aria-pressed={selected}
          className={`plan-board-switcher-btn${selected ? " plan-board-switcher-btn-active" : ""}`}
          title={tip}
          onClick={() => onSelect(plan.id)}
        >
          {plan.title}
          {inflightBadge(plan)}
        </button>
        <KebabTrigger
          plan={plan}
          isOpen={menuOpen}
          onToggle={(e) => toggleMenu(plan, e)}
          refCallback={(el) => {
            if (el) kebabEls.current.set(plan.id, el);
            else kebabEls.current.delete(plan.id);
          }}
        />
        {menuOpen && openMenu && (
          <KebabMenu
            plan={plan}
            onAction={onAction}
            onClose={() => setOpenMenu(null)}
            triggerEl={openMenu.el}
          />
        )}
      </div>
    );
  };

  return (
    <>
      <div className="plan-board-switcher-seg">
        {activePlans.map((p) => makePill(p))}
      </div>
      <ArchivedPlansDropdown
        plans={archivedPlans}
        selectedId={selectedId}
        onSelect={onSelect}
        onAction={onAction}
      />
    </>
  );
}

function ArchivedPlansDropdown({
  plans,
  selectedId,
  onSelect,
  onAction,
}: {
  plans: readonly PlanSwitcherOption[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onAction: (a: PlanKebabAction) => void;
}) {
  const [open, setOpen] = useState(false);
  const [openMenu, setOpenMenu] = useState<{ id: string; el: HTMLButtonElement } | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listboxRef = useRef<HTMLDivElement>(null);
  const kebabEls = useRef<Map<string, HTMLButtonElement>>(new Map());
  const selectedArchivedPlan = plans.find((p) => p.id === selectedId);

  const close = useCallback(() => {
    setOpen(false);
    setOpenMenu(null);
  }, []);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (
        !listboxRef.current?.contains(e.target as Node) &&
        !triggerRef.current?.contains(e.target as Node)
      ) {
        close();
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open, close]);

  if (plans.length === 0) return null;

  const onListboxKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
      triggerRef.current?.focus();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const btns = Array.from(
      listboxRef.current?.querySelectorAll<HTMLButtonElement>(".plan-switcher-opt-select") ?? [],
    );
    const cur = btns.indexOf(document.activeElement as HTMLButtonElement);
    const next =
      e.key === "ArrowDown"
        ? cur === -1
          ? 0
          : Math.min(cur + 1, btns.length - 1)
        : cur === -1
          ? btns.length - 1
          : Math.max(cur - 1, 0);
    btns[next]?.focus();
  };

  const selectAndClose = (id: string) => {
    onSelect(id);
    close();
  };

  const toggleMenu = (plan: PlanSwitcherOption, e: React.MouseEvent) => {
    e.stopPropagation();
    const el = kebabEls.current.get(plan.id);
    if (!el) return;
    setOpenMenu((prev) => (prev?.id === plan.id ? null : { id: plan.id, el }));
  };

  return (
    <div className="plan-switcher-archived-dropdown" data-testid="plan-switcher-archived-dropdown">
      <button
        ref={triggerRef}
        type="button"
        data-testid="plan-switcher-archived-toggle"
        className={`plan-switcher-archived-toggle${selectedArchivedPlan ? " plan-switcher-archived-toggle-selected" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        {selectedArchivedPlan ? `Archived: ${selectedArchivedPlan.title}` : `${plans.length} archived`}
        <span className="plan-switcher-combo-caret" aria-hidden>
          {open ? " ▴" : " ▾"}
        </span>
      </button>
      {open && (
        <div
          ref={listboxRef}
          role="listbox"
          data-testid="plan-switcher-archived-list"
          className="ct-menu plan-switcher-popover plan-switcher-archived-popover"
          aria-label="Archived plans"
          onKeyDown={onListboxKeyDown}
        >
          {plans.map((plan) => {
            const active = plan.id === selectedId;
            const menuOpen = openMenu?.id === plan.id;
            return (
              <div
                key={plan.id}
                role="option"
                aria-selected={active}
                data-plan-id={plan.id}
                className={`plan-switcher-opt plan-switcher-opt-archived${active ? " plan-switcher-opt-selected" : ""}`}
              >
                <button
                  type="button"
                  className="ct-menu-row plan-switcher-opt-select"
                  title={plan.inflightTooltip ? `${plan.title}\n${plan.inflightTooltip}` : plan.title}
                  onClick={() => selectAndClose(plan.id)}
                >
                  <span className="min-w-0 flex-1 truncate">{plan.title}</span>
                  {inflightBadge(plan)}
                </button>
                <KebabTrigger
                  plan={plan}
                  isOpen={menuOpen}
                  onToggle={(e) => toggleMenu(plan, e)}
                  refCallback={(el) => {
                    if (el) kebabEls.current.set(plan.id, el);
                    else kebabEls.current.delete(plan.id);
                  }}
                />
                {menuOpen && openMenu && (
                  <KebabMenu
                    plan={plan}
                    onAction={onAction}
                    onClose={() => setOpenMenu(null)}
                    triggerEl={openMenu.el}
                  />
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function OverflowSwitcher({
  activePlans,
  archivedPlans,
  selectedId,
  onSelect,
  onAction,
}: {
  activePlans: readonly PlanSwitcherOption[];
  archivedPlans: readonly PlanSwitcherOption[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onAction: (a: PlanKebabAction) => void;
}) {
  const [listboxOpen, setListboxOpen] = useState(false);
  const [openMenu, setOpenMenu] = useState<{ id: string; el: HTMLButtonElement } | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listboxRef = useRef<HTMLDivElement>(null);
  const kebabEls = useRef<Map<string, HTMLButtonElement>>(new Map());

  const selectedPlan =
    activePlans.find((p) => p.id === selectedId) ?? archivedPlans.find((p) => p.id === selectedId);

  const close = useCallback(() => {
    setListboxOpen(false);
    setOpenMenu(null);
  }, []);

  useEffect(() => {
    if (!listboxOpen) return;
    const onDown = (e: MouseEvent) => {
      if (
        !listboxRef.current?.contains(e.target as Node) &&
        !triggerRef.current?.contains(e.target as Node)
      ) {
        close();
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [listboxOpen, close]);

  const onListboxKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
      triggerRef.current?.focus();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const btns = Array.from(
      listboxRef.current?.querySelectorAll<HTMLButtonElement>(
        ".plan-switcher-opt-select:not(:disabled)",
      ) ?? [],
    );
    const cur = btns.indexOf(document.activeElement as HTMLButtonElement);
    const next =
      e.key === "ArrowDown"
        ? cur === -1
          ? 0
          : Math.min(cur + 1, btns.length - 1)
        : cur === -1
          ? btns.length - 1
          : Math.max(cur - 1, 0);
    btns[next]?.focus();
  };

  const selectAndClose = (id: string) => {
    onSelect(id);
    setListboxOpen(false);
    setOpenMenu(null);
  };

  const toggleMenu = (plan: PlanSwitcherOption, e: React.MouseEvent) => {
    e.stopPropagation();
    const el = kebabEls.current.get(plan.id);
    if (!el) return;
    setOpenMenu((prev) => (prev?.id === plan.id ? null : { id: plan.id, el }));
  };

  const renderRow = (plan: PlanSwitcherOption, isArchived: boolean) => {
    const active = plan.id === selectedId;
    const menuOpen = openMenu?.id === plan.id;

    return (
      <div
        key={plan.id}
        role="option"
        aria-selected={active}
        data-plan-id={plan.id}
        className={`plan-switcher-opt${active ? " plan-switcher-opt-selected" : ""}${isArchived ? " plan-switcher-opt-archived" : ""}`}
      >
        <button
          type="button"
          className="ct-menu-row plan-switcher-opt-select"
          title={plan.inflightTooltip ? `${plan.title}\n${plan.inflightTooltip}` : plan.title}
          onClick={() => selectAndClose(plan.id)}
        >
          <span className="min-w-0 flex-1 truncate">{plan.title}</span>
          {inflightBadge(plan)}
        </button>
        <KebabTrigger
          plan={plan}
          isOpen={menuOpen}
          onToggle={(e) => toggleMenu(plan, e)}
          refCallback={(el) => {
            if (el) kebabEls.current.set(plan.id, el);
            else kebabEls.current.delete(plan.id);
          }}
        />
        {menuOpen && openMenu && (
          <KebabMenu
            plan={plan}
            onAction={onAction}
            onClose={() => setOpenMenu(null)}
            triggerEl={openMenu.el}
          />
        )}
      </div>
    );
  };

  return (
    <div className="plan-switcher-combo">
      <button
        ref={triggerRef}
        type="button"
        data-testid="plan-switcher-combo-trigger"
        className="plan-switcher-combo-trigger"
        aria-haspopup="listbox"
        aria-expanded={listboxOpen}
        onClick={() => setListboxOpen((v) => !v)}
      >
        {selectedPlan?.title ?? "Select plan"}
        <span className="plan-switcher-combo-caret" aria-hidden>
          {listboxOpen ? " ▴" : " ▾"}
        </span>
      </button>
      {listboxOpen && (
        <div
          ref={listboxRef}
          role="listbox"
          data-testid="plan-switcher-listbox"
          className="ct-menu plan-switcher-popover"
          aria-label="Select plan"
          onKeyDown={onListboxKeyDown}
        >
          {activePlans.map((p) => renderRow(p, false))}
          {archivedPlans.length > 0 && (
            <>
              <div className="ct-menu-separator" aria-hidden />
              <div className="ct-menu-caption" aria-hidden>
                Archived
              </div>
              {archivedPlans.map((p) => renderRow(p, true))}
            </>
          )}
        </div>
      )}
    </div>
  );
}

/** Shared single-plan switcher for Board and List: segmented ≤3 active, custom popover beyond. */
export function PlanSwitcher({
  plans,
  selectedId,
  onSelect,
  onAction,
  label = "Plan",
}: PlanSwitcherProps) {
  // All hooks must come before any early returns.
  const activePlans = useMemo(() => plans.filter((p) => !p.archived), [plans]);
  const archivedPlans = useMemo(() => plans.filter((p) => p.archived), [plans]);

  const handleAction = useCallback(
    (action: PlanKebabAction) => {
      if (action.kind === "archive" && action.planId === selectedId) {
        const remaining = activePlans.filter((p) => p.id !== action.planId);
        if (remaining.length > 0) onSelect(remaining[0].id);
      }
      onAction?.(action);
    },
    [selectedId, activePlans, onSelect, onAction],
  );

  if (activePlans.length <= 1 && archivedPlans.length === 0) return null;
  if (plans.length <= 1) return null;

  return (
    <div
      data-testid="plan-switcher"
      className="plan-board-switcher"
      role="group"
      aria-label="Select plan"
    >
      <span className="plan-board-switcher-label">{label}</span>
      {activePlans.length <= SEGMENTED_MAX ? (
        <SegmentedSwitcher
          activePlans={activePlans}
          archivedPlans={archivedPlans}
          selectedId={selectedId}
          onSelect={onSelect}
          onAction={handleAction}
        />
      ) : (
        <OverflowSwitcher
          activePlans={activePlans}
          archivedPlans={archivedPlans}
          selectedId={selectedId}
          onSelect={onSelect}
          onAction={handleAction}
        />
      )}
    </div>
  );
}
