import { ArrowDownToLine } from "lucide-react";
import { useUpdate } from "../hooks/useUpdate";
import { BusyState } from "./shared/BusyState";

export function UpdateBadge() {
  const { status, latestVersion, installing, progress, applyUpdate } = useUpdate();
  const version = knownVersion(latestVersion);

  if (status === "available" && !installing) {
    const label = version ? `Update ${version}` : "Updates available";
    return (
      <button
        type="button"
        onClick={() => void applyUpdate()}
        aria-label={label}
        title={version ? `Install update v${version}` : label}
        className="group inline-flex items-center gap-1 h-[22px] shrink-0 rounded-full pl-1.5 pr-2 text-[11px] font-medium leading-none text-white transition-[filter,transform,box-shadow] duration-150 hover:brightness-110 hover:-translate-y-px active:translate-y-0"
        style={{
          // Subtle top-lit accent — theme-respecting (derives from --ct-accent), reads as a polished pill rather than a flat block.
          background:
            "linear-gradient(180deg, color-mix(in srgb, var(--ct-accent, #3b82f6) 84%, #fff 16%), var(--ct-accent, #3b82f6))",
          border: "1px solid color-mix(in srgb, #fff 18%, transparent)",
          boxShadow: "0 1px 2px rgba(0,0,0,0.28)",
        }}
      >
        <ArrowDownToLine
          size={12}
          strokeWidth={2.4}
          className="shrink-0 transition-transform duration-150 group-hover:translate-y-px"
        />
        <span>{label}</span>
      </button>
    );
  }

  if (installing) {
    return (
      <span
        role="status"
        aria-label="Updating"
        title="Installing update"
        className="inline-flex items-center gap-1.5 h-[22px] shrink-0 rounded-full px-2 text-[11px] font-medium leading-none text-[var(--ct-muted-fg)]"
        style={{
          background: "var(--ct-titlebar-bg, #161b22)",
          border: "1px solid var(--ct-border, #30363d)",
        }}
      >
        <Spinner />
        <span className="tabular-nums">{Math.max(1, Math.round(progress))}%</span>
      </span>
    );
  }

  return null;
}

function knownVersion(value: string | null): string | null {
  const version = value?.trim();
  return version && /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)
    ? version
    : null;
}

function Spinner() {
  return <BusyState inline hideLabel label="Updating" phase="work" dotSize="micro" />;
}
