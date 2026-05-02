import { useUpdate } from "../hooks/useUpdate";

interface UpdateBadgeProps {
  onNavigate?: () => void;
}

export function UpdateBadge({ onNavigate }: UpdateBadgeProps) {
  const { status, latestVersion, applying } = useUpdate();

  if (status === "available" && latestVersion) {
    return (
      <button
        onClick={onNavigate}
        title={`Update available: v${latestVersion}`}
        className="flex items-center gap-1.5 h-6 px-2 rounded text-[10px] font-medium text-white bg-[var(--ct-accent)] hover:opacity-90 transition-opacity"
      >
        <span className="w-1.5 h-1.5 rounded-full bg-white" />
        <span>Update v{latestVersion}</span>
      </button>
    );
  }

  if (applying) {
    return (
      <span
        title="Installing update"
        className="flex items-center gap-1 h-6 px-2 text-[10px] text-[var(--ct-muted-fg)]"
      >
        <Spinner />
        <span>Updating</span>
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
