import { ArrowDownToLine } from "lucide-react";
import { useUpdate } from "../hooks/useUpdate";

export function UpdateBadge() {
  const { status, latestVersion, installing, progress, applyUpdate } = useUpdate();

  if (status === "available" && latestVersion && !installing) {
    return (
      <button
        type="button"
        onClick={() => void applyUpdate()}
        title={`Install update v${latestVersion}`}
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
        <span>Update {latestVersion}</span>
      </button>
    );
  }

  if (installing) {
    return (
      <span
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

function Spinner() {
  return (
    <svg
      width="10"
      height="10"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      className="animate-spin"
    >
      <path d="M8 2 a6 6 0 0 1 6 6" strokeLinecap="round" />
    </svg>
  );
}
