import { useEffect, useRef, useState } from "react";
import type { Specimen } from "./types";
import { groupSpecimens } from "./navigationModel";

export function useGalleryNavigation(specimens: Specimen[], activeId?: string) {
  const [query, setQuery] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const groups = groupSpecimens(specimens, query);
  const ordered = groupSpecimens(specimens).flatMap((group) => group.entries);
  const index = ordered.findIndex((specimen) => specimen.id === activeId);
  const previous = ordered[index - 1];
  const next = index >= 0 ? ordered[index + 1] : undefined;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.ctrlKey || event.metaKey || event.altKey || event.isComposing) return;
      const target = event.target;
      if (target instanceof HTMLElement && (target.closest("input, textarea, select") || target.isContentEditable)) return;
      if (event.code === "Slash") {
        event.preventDefault();
        searchRef.current?.focus();
      }
      const destination = event.code === "BracketLeft" ? previous : event.code === "BracketRight" ? next : undefined;
      if (destination) {
        event.preventDefault();
        window.location.hash = `gallery/${destination.id}`;
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [previous, next]);

  return { query, setQuery, searchRef, groups, previous, next };
}
