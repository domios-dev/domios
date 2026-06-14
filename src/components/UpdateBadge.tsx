import { useUpdate } from "../hooks/useUpdate";
import { UI_BUTTON } from "./shared/design";

export function UpdateBadge() {
  const { status, latestVersion, installing, progress, applyUpdate } = useUpdate();

  if (status === "available" && latestVersion && !installing) {
    return (
      <button
        type="button"
        onClick={() => void applyUpdate()}
        title={`Install update v${latestVersion}`}
        className={`${UI_BUTTON.primary} ${UI_BUTTON.sm}`}
      >
        <span className="w-1.5 h-1.5 rounded-full bg-white" />
        <span>Update v{latestVersion}</span>
      </button>
    );
  }

  if (installing) {
    return (
      <span
        title="Installing update"
        className={`${UI_BUTTON.base} ${UI_BUTTON.sm} ct-btn-ghost`}
      >
        <Spinner />
        <span>{Math.max(1, Math.round(progress))}%</span>
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
