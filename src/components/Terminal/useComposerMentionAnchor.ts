import { useLayoutEffect, useState, type RefObject } from "react";
import { useTouchOptimizedUi } from "../../hooks/useTouchOptimizedUi";

const MIRROR_PREFIX_LIMIT = 32_768;
const TEXT_LAYOUT_PROPERTIES = [
  "font-family", "font-size", "font-weight", "font-style", "font-stretch", "font-variant",
  "line-height", "letter-spacing", "word-spacing", "text-indent", "text-transform", "text-align",
  "direction", "tab-size", "word-break", "overflow-wrap",
  "padding-top", "padding-right", "padding-bottom", "padding-left",
  "border-top-width", "border-right-width", "border-bottom-width", "border-left-width",
] as const;

export function useComposerMentionAnchor(
  textarea: RefObject<HTMLTextAreaElement | null>, active: boolean, start: number | undefined, session: string,
): { x: number; y: number } | undefined {
  const touchUi = useTouchOptimizedUi();
  const [point, setPoint] = useState<{ x: number; y: number }>();
  useLayoutEffect(() => {
    const editor = textarea.current;
    if (!active || touchUi || start === undefined || start > MIRROR_PREFIX_LIMIT || !editor) {
      setPoint(undefined);
      return;
    }
    const mirror = editor.ownerDocument.createElement("div");
    mirror.setAttribute("aria-hidden", "true");
    mirror.style.cssText = "position:fixed;left:0;top:0;visibility:hidden;pointer-events:none;box-sizing:border-box;overflow:hidden;border-style:solid;";
    const marker = editor.ownerDocument.createElement("span");
    let offset: { x: number; y: number } | undefined;
    const update = () => {
      if (!offset) { setPoint(undefined); return; }
      const rect = editor.getBoundingClientRect();
      const next = { x: rect.left + offset.x - editor.scrollLeft, y: rect.top + offset.y - editor.scrollTop };
      setPoint(previous => previous?.x === next.x && previous.y === next.y ? previous : next);
    };
    const measure = () => {
      if (editor.value[start] !== "@" || editor.clientWidth <= 0) {
        offset = undefined;
        update();
        return;
      }
      const style = getComputedStyle(editor);
      for (const property of TEXT_LAYOUT_PROPERTIES) mirror.style.setProperty(property, style.getPropertyValue(property));
      const borders = Number.parseFloat(style.borderLeftWidth) + Number.parseFloat(style.borderRightWidth);
      mirror.style.width = `${editor.clientWidth + (Number.isFinite(borders) ? borders : 0)}px`;
      mirror.style.whiteSpace = editor.wrap === "off" ? "pre" : "pre-wrap";
      mirror.textContent = editor.value.slice(0, start);
      marker.textContent = "@";
      mirror.append(marker);
      const markerRect = marker.getBoundingClientRect();
      const mirrorRect = mirror.getBoundingClientRect();
      offset = markerRect.height > 0 ? { x: markerRect.left - mirrorRect.left, y: markerRect.top - mirrorRect.top } : undefined;
      update();
    };
    editor.ownerDocument.body.append(mirror);
    measure();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    observer?.observe(editor);
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", update, true);
    window.visualViewport?.addEventListener("resize", measure);
    window.visualViewport?.addEventListener("scroll", update);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", measure);
      window.removeEventListener("scroll", update, true);
      window.visualViewport?.removeEventListener("resize", measure);
      window.visualViewport?.removeEventListener("scroll", update);
      mirror.remove();
    };
  }, [active, start, session, textarea, touchUi]);
  return point;
}
