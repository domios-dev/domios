import { useState } from "react";
import { useUpdate } from "../hooks/useUpdate";
import { UpdateModal } from "./UpdateModal";

export function UpdateBadge() {
  const update = useUpdate();
  const [open, setOpen] = useState(false);

  const { status, latestVersion } = update;

  const onClick = () => setOpen(true);

  if (status === "checking") {
    return (
      <button
        onClick={onClick}
        title="Checking for updates"
        className="flex items-center gap-1 h-6 px-2 rounded text-[10px] text-[var(--ct-muted)] hover:bg-[var(--ct-hover)] transition-colors"
      >
        <Spinner />
        <span>Checking</span>
      </button>
    );
  }

  if (status === "available" && latestVersion) {
    return (
      <>
        <button
          onClick={onClick}
          title={`Update available: v${latestVersion}`}
          className="flex items-center gap-1.5 h-6 px-2 rounded text-[10px] font-medium text-white bg-[var(--ct-accent)] hover:opacity-90 transition-opacity"
        >
          <span className="w-1.5 h-1.5 rounded-full bg-white" />
          <span>Update v{latestVersion}</span>
        </button>
        {open && <UpdateModal update={update} onClose={() => setOpen(false)} />}
      </>
    );
  }

  if (status === "applying") {
    return (
      <button
        onClick={onClick}
        title="Installing update"
        className="flex items-center gap-1 h-6 px-2 rounded text-[10px] text-[var(--ct-muted-fg)]"
      >
        <Spinner />
        <span>Updating</span>
      </button>
    );
  }

  if (status === "error") {
    return (
      <button
        onClick={() => update.refresh(true)}
        title="Update check failed — click to retry"
        className="flex items-center gap-1 h-6 px-2 rounded text-[10px] text-[var(--ct-red)] hover:bg-[var(--ct-hover)] transition-colors"
      >
        <span className="w-1.5 h-1.5 rounded-full bg-[var(--ct-red)]" />
        <span>Update?</span>
      </button>
    );
  }

  return (
    <button
      onClick={onClick}
      title={`Up to date${update.currentVersion ? ` (v${update.currentVersion})` : ""}`}
      className="flex items-center justify-center w-6 h-6 rounded hover:bg-[var(--ct-hover)] transition-colors"
    >
      <span className="w-1.5 h-1.5 rounded-full bg-[var(--ct-green)] opacity-60" />
      {open && <UpdateModal update={update} onClose={() => setOpen(false)} />}
    </button>
  );
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
