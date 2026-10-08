import { Bug, Check, Circle, Hammer, Server, Terminal, X } from "lucide-react";
import { DynamicIcon, type IconName } from "lucide-react/dynamic";
import { useRef } from "react";
import { useHammerStrike } from "./useHammerStrike";
import { BusyState } from "./BusyState";
import type { Tab } from "../../lib/types";
import { shellPurposeLabel, shellStateLabel, type ShellPurpose } from "../../lib/shellCommandState";

const purposeIcons = { build: Hammer, dev: Server, qa: Bug, shell: Terminal };
const defaultNames = { build: "hammer", dev: "server", qa: "bug", shell: "terminal" };

export function ShellPurposeIcon({ purpose = "shell", running = false, icon }: { purpose?: ShellPurpose; running?: boolean; icon?: string | null }) {
  const Icon = purposeIcons[purpose];
  const iconRef = useRef<SVGSVGElement>(null);
  const custom = icon && icon !== defaultNames[purpose];
  useHammerStrike(iconRef, !custom && (purpose === "build" || purpose === "qa") && running, purpose);
  if (custom) return <span role="img" aria-label={`${icon}, ${shellPurposeLabel(purpose)}`}>
    <DynamicIcon name={icon as IconName} aria-hidden="true" size={14} />
  </span>;
  return <Icon ref={iconRef} role="img" aria-label={shellPurposeLabel(purpose)} size={14} />;
}

export function ShellStatus({ tab }: { tab: Tab }) {
  const state = tab.shell_command_state;
  const label = shellStateLabel(state);
  const running = state?.activity === "running";
  const Icon = state?.activity === "idle" && state.last_exit_code != null
    ? state.last_exit_code === 0 ? Check : X : Circle;
  return (
    <span role="status" aria-label={`${shellPurposeLabel(tab.purpose)}, ${tab.title}: ${label}`}
      className="inline-flex max-w-full items-center gap-1 text-[10px] text-[var(--ct-muted)]">
      {running ? <BusyState inline hideLabel label={label} /> : <Icon aria-hidden="true" size={12} className="shrink-0" />}
      <span className="truncate">{shellPurposeLabel(tab.purpose)} · {label}</span>
    </span>
  );
}
