import { useLayoutEffect, useRef } from "react";
import { DragOverlay, useDndContext } from "@dnd-kit/core";

export function SidebarDragPreview() {
  const { active } = useDndContext();
  return <DragOverlay dropAnimation={null}>
    {active ? <DragSnapshot id={String(active.id)} /> : null}
  </DragOverlay>;
}

function DragSnapshot({ id }: { id: string }) {
  const host = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (!host.current) return;
    const source = document.querySelector<HTMLElement>(`[data-dnd-id="${globalThis.CSS.escape(id)}"]`);
    if (!source) return;
    const clone = source.cloneNode(true) as HTMLElement;
    clone.style.opacity = "1";
    for (const node of [clone, ...clone.querySelectorAll("*")]) {
      for (const attribute of ["id", "data-dnd-id", "data-dnd-group", "data-dnd-dragging"]) node.removeAttribute(attribute);
    }
    const container = host.current;
    container.replaceChildren(clone);
    return () => container.replaceChildren();
  }, [id]);
  return <div ref={host} aria-hidden="true" inert style={{
    pointerEvents: "none", background: "var(--ct-bg)", borderRadius: 6, boxShadow: "0 3px 8px rgb(0 0 0 / 18%)",
  }} />;
}
