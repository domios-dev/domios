import { tabActivity } from "../../lib/tabActivity";
import { Bug, Check, Circle, Database, Rocket, ScrollText, Server, Terminal, X } from "lucide-react";
import { DynamicIcon, type IconName } from "lucide-react/dynamic";
import { type RefObject, useEffect, useRef } from "react";
import { BuildShellIcon } from "./BuildShellIcon";
import { BusyState } from "./BusyState";
import type { Tab } from "../../lib/types";
import { shellPurposeLabel, type ShellPurpose } from "../../lib/shellCommandState";

const purposeIcons = { dev: Server, qa: Bug, deploy: Rocket, logs: ScrollText, db: Database, shell: Terminal };
const defaultNames = { build: "hammer", dev: "server", qa: "bug", deploy: "rocket", logs: "scroll-text", db: "database", shell: "terminal" };

function useBugWiggle(iconRef: RefObject<SVGSVGElement | null>, active: boolean) {
  useEffect(() => {
    const icon = iconRef.current;
    if (!active || !icon || typeof icon.animate !== "function") return;
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    let animation: Animation | undefined;
    const update = () => {
      animation?.cancel();
      animation = motion.matches ? undefined : icon.animate(
        [{ transform: "rotate(-6deg)" }, { transform: "rotate(6deg)" }, { transform: "rotate(-6deg)" }],
        { duration: 1200, iterations: Infinity, easing: "ease-in-out" },
      );
    };
    update();
    motion.addEventListener("change", update);
    return () => { animation?.cancel(); motion.removeEventListener("change", update); };
  }, [active, iconRef]);
}

export function ShellPurposeIcon({ purpose = "shell", running = false, icon }: { purpose?: ShellPurpose; running?: boolean; icon?: string | null }) {
  const iconRef = useRef<SVGSVGElement>(null);
  const custom = Boolean(icon) && icon !== defaultNames[purpose];
  useBugWiggle(iconRef, !custom && purpose === "qa" && running);
  if (custom) return <span role="img" aria-label={`${icon}, ${shellPurposeLabel(purpose)}`}>
    <DynamicIcon name={icon as IconName} aria-hidden="true" size={14} />
  </span>;
  if (purpose === "build") return <BuildShellIcon running={running} />;
  const Icon = purposeIcons[purpose];
  return <Icon ref={iconRef} role="img" aria-label={shellPurposeLabel(purpose)} size={14} />;
}

export function ShellStatus({ tab }: { tab: Tab }) {
  const activity = tabActivity(tab);
  const label = activity.label;
  const running = activity.active;
  const Icon = activity.shellSucceeded === null ? Circle : activity.shellSucceeded ? Check : X;
  return (
    <span role="status" aria-label={`${shellPurposeLabel(tab.purpose)}, ${tab.title}: ${label}`}
      className="inline-flex max-w-full items-center gap-1 text-[10px] text-[var(--ct-muted)]">
      {running ? <BusyState activity={activity} inline hideLabel /> : <Icon aria-hidden="true" size={12} className="shrink-0" />}
      <span className="truncate">{shellPurposeLabel(tab.purpose)} · {label}</span>
    </span>
  );
}
