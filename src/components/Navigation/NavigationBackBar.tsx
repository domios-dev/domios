import { useEffect } from "react";
import { useNavigationReturn, navigationStore } from "./navigationStore";

interface Props {
  onBack: () => void;
}

export function NavigationBackBar({ onBack }: Props) {
  const nav = useNavigationReturn();

  useEffect(() => {
    if (!nav) return;
    const handler = (e: KeyboardEvent) => {
      if (e.altKey && e.code === "ArrowLeft") {
        e.preventDefault();
        onBack();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [nav, onBack]);

  if (!nav) return null;

  return (
    <button
      onClick={() => {
        onBack();
        navigationStore.clear();
      }}
      className="fixed top-11 left-1/2 -translate-x-1/2 z-[1100] flex items-center gap-1.5 px-3.5 py-1.5 rounded-full text-xs cursor-pointer bg-[var(--ct-surface)] border border-[var(--ct-border)] text-[var(--ct-fg)] shadow-[0_4px_14px_rgba(0,0,0,0.4)]"
      title="Back (Alt+Left)"
    >
      <span>←</span>
      <span>Back to {nav.label}</span>
    </button>
  );
}
