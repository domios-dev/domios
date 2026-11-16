import { useLayoutEffect, useRef } from "react";

export function useSelectedTemplateCard(selected: boolean, reason: string | undefined) {
  const card = useRef<HTMLLIElement>(null);
  useLayoutEffect(() => {
    if (selected && reason) card.current?.scrollIntoView({ block: "nearest", behavior: "instant" });
  }, [selected, reason]);
  return card;
}
