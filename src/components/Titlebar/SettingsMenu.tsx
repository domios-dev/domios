import { useState } from "react";
import { ArrowDownToLine } from "lucide-react";
import { GearIcon } from "../Icons";
import { BusySpinner } from "../shared/BusyState";
import { DropdownMenu, type DropdownMenuItem } from "../shared/DropdownMenu";
import { UI_TRAY_BTN } from "../shared/design";
import { SETTINGS_SECTIONS, type SectionId } from "../Settings/sections";
import { useUpdate } from "../../hooks/useUpdate";

interface SettingsMenuProps {
  onOpenSection: (section?: SectionId) => void;
}

/**
 * Gear button in the titlebar tray that opens the settings menu.
 * Each row deep-links into the full settings panel at that section.
 */
export function SettingsMenu({ onOpenSection }: SettingsMenuProps) {
  const [open, setOpen] = useState(false);
  const { status, latestVersion, installing, progress, applyUpdate } = useUpdate();

  const realUpdate = status === "available" && !!latestVersion && !installing;
  const showUpdate = realUpdate || installing;

  const items: DropdownMenuItem[] = SETTINGS_SECTIONS.map((s) => {
    const Icon = s.icon;
    return {
      label: s.label,
      description: s.desc,
      icon: <Icon size={14} />,
      onClick: () => onOpenSection(s.id),
      "data-testid": `settings-menu-${s.id}`,
    };
  });

  const gear = (className: string) => (
    <DropdownMenu
      items={items}
      menuLabel="Settings"
      menuTestId="settings-menu"
      onOpenChange={setOpen}
      className="flex"
      trigger={
        <button
          type="button"
          data-testid="titlebar-settings"
          title="Settings"
          aria-haspopup="menu"
          aria-expanded={open}
          className={className}
        >
          <GearIcon size={15} className={open ? "rotate-45 transition-transform" : "transition-transform"} />
        </button>
      }
    />
  );

  const updatePill = (
    <button
      type="button"
      data-testid="settings-update-action"
      onClick={() => void applyUpdate()}
      disabled={installing}
      title={installing ? "Installing update" : `Install update${latestVersion ? ` v${latestVersion}` : ""}`}
      className="group inline-flex items-center gap-1 h-6 rounded-full pl-1.5 pr-2 text-[11px] font-medium leading-none text-[var(--ct-on-accent)] transition-[filter,transform] duration-150 hover:brightness-110 disabled:opacity-70"
      style={{
        background:
          "linear-gradient(180deg, color-mix(in srgb, var(--ct-accent, #3b82f6) 84%, var(--ct-sheen) 16%), var(--ct-accent, #3b82f6))",
        border: "1px solid color-mix(in srgb, var(--ct-sheen) 18%, transparent)",
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
    </>
  );
}

function Spinner() {
  return <BusySpinner size={10} />;
}
