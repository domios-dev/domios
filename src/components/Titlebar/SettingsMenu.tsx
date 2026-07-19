import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ArrowDownToLine, ChevronRight } from "lucide-react";
import { GearIcon } from "../Icons";
import { BusySpinner } from "../shared/BusyState";
import { UI_TRAY_BTN } from "../shared/design";
import { SETTINGS_SECTIONS, type SectionId } from "../Settings/sections";
import { useUpdate } from "../../hooks/useUpdate";

interface SettingsMenuProps {
  onOpenSection: (section?: SectionId) => void;
}

const MENU_WIDTH = 288;


/**
 * Gear button in the titlebar tray that opens a chunky settings menu.
 * Each row deep-links into the full settings panel at that section.
 */
export function SettingsMenu({ onOpenSection }: SettingsMenuProps) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ top: number; right: number } | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const { status, latestVersion, installing, progress, applyUpdate } = useUpdate();

  const realUpdate = status === "available" && !!latestVersion && !installing;
  const showUpdate = realUpdate || installing;

  const place = useCallback(() => {
    const r = triggerRef.current?.getBoundingClientRect();
    if (!r) return;
    setPos({ top: r.bottom + 6, right: Math.max(8, window.innerWidth - r.right) });
  }, []);

  const toggle = useCallback(() => {
    setOpen((v) => {
      if (!v) place();
      return !v;
    });
  }, [place]);

  const choose = useCallback(
    (section?: SectionId) => {
      setOpen(false);
      onOpenSection(section);
    },
    [onOpenSection],
  );

  useEffect(() => {
    if (!open) return;
    const onPointer = (e: MouseEvent) => {
      if (
        !menuRef.current?.contains(e.target as Node) &&
        !triggerRef.current?.contains(e.target as Node)
      ) {
        setOpen(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onPointer, true);
    document.addEventListener("keydown", onKey);
    window.addEventListener("resize", place);
    return () => {
      document.removeEventListener("mousedown", onPointer, true);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", place);
    };
  }, [open, place]);

  const gear = (className: string) => (
    <button
      ref={triggerRef}
      type="button"
      data-testid="titlebar-settings"
      onClick={toggle}
      title="Settings"
      aria-haspopup="menu"
      aria-expanded={open}
      className={className}
    >
      <GearIcon size={15} className={open ? "rotate-45 transition-transform" : "transition-transform"} />
    </button>
  );

  const updatePill = (
    <button
      type="button"
      data-testid="settings-update-action"
      onClick={() => void applyUpdate()}
      disabled={installing}
      title={installing ? "Installing update" : `Install update${latestVersion ? ` v${latestVersion}` : ""}`}
      className="group inline-flex items-center gap-1 h-6 rounded-full pl-1.5 pr-2 text-[11px] font-medium leading-none text-white transition-[filter,transform] duration-150 hover:brightness-110 disabled:opacity-70"
      style={{
        background:
          "linear-gradient(180deg, color-mix(in srgb, var(--ct-accent, #3b82f6) 84%, #fff 16%), var(--ct-accent, #3b82f6))",
        border: "1px solid color-mix(in srgb, #fff 18%, transparent)",
        boxShadow: "0 1px 2px rgba(0,0,0,0.28)",
      }}
    >
      {installing ? (
        <>
          <Spinner />
          <span className="tabular-nums">{Math.max(1, Math.round(progress))}%</span>
        </>
      ) : (
        <>
          <ArrowDownToLine
            size={12}
            strokeWidth={2.4}
            className="shrink-0 transition-transform duration-150 group-hover:translate-y-px"
          />
          <span>Update</span>
        </>
      )}
    </button>
  );

  return (
    <>
      {showUpdate ? (
        <div
          data-testid="settings-update-capsule"
          className="flex items-center gap-1 h-7 pl-1 pr-1 rounded-full border border-[var(--ct-border-subtle,rgba(255,255,255,0.08))] bg-[var(--ct-bg-elev,rgba(255,255,255,0.03))]"
        >
          {gear(
            `flex items-center justify-center w-6 h-6 rounded-full transition-colors ${
              open
                ? "bg-[var(--ct-hover)] text-[var(--ct-fg)]"
                : "text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
            }`,
          )}
          {updatePill}
        </div>
      ) : (
        gear(`${UI_TRAY_BTN} ${open ? "bg-[var(--ct-hover)] text-[var(--ct-fg)]" : ""}`)
      )}

      {open &&
        pos &&
        createPortal(
          <div
            ref={menuRef}
            role="menu"
            data-testid="settings-menu"
            className="ct-menu-pop ct-float ct-portal-menu fixed overflow-hidden rounded-2xl border border-[var(--ct-border)] bg-[var(--ct-surface)] shadow-[var(--ct-shadow-lg)]"
            style={{ top: pos.top, right: pos.right, width: MENU_WIDTH }}
          >
            <div className="flex items-center justify-between px-3.5 pt-3 pb-2">
              <span className="text-[10px] font-semibold uppercase tracking-[0.14em] text-[var(--ct-muted)]">
                Settings
              </span>
            </div>

            <div className="px-1.5 pb-1.5">
              {SETTINGS_SECTIONS.map((s) => {
                const Icon = s.icon;
                return (
                  <button
                    key={s.id}
                    type="button"
                    role="menuitem"
                    data-testid={`settings-menu-${s.id}`}
                    onClick={() => choose(s.id)}
                    className="group relative flex w-full items-center gap-3 rounded-xl px-2.5 py-2.5 text-left transition-colors hover:bg-[var(--ct-hover)]"
                  >
                    <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-[color-mix(in_srgb,var(--ct-accent)_14%,transparent)] text-[var(--ct-accent)] transition-colors group-hover:bg-[color-mix(in_srgb,var(--ct-accent)_24%,transparent)]">
                      <Icon size={16} strokeWidth={2} />
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block text-[13px] font-medium leading-tight text-[var(--ct-fg)]">{s.label}</span>
                      <span className="mt-0.5 block truncate text-[11px] leading-tight text-[var(--ct-muted-fg)]">{s.desc}</span>
                    </span>
                    <ChevronRight
                      size={14}
                      className="shrink-0 text-[var(--ct-muted)] transition-transform duration-150 group-hover:translate-x-0.5 group-hover:text-[var(--ct-fg)]"
                    />
                  </button>
                );
              })}
            </div>
          </div>,
          document.body,
        )}
    </>
  );
}

function Spinner() {
  return <BusySpinner size={10} />;
}
