import { useCallback, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { useKeyboardInset } from "../../hooks/useKeyboardInset";
import { compositionDefersAutosize, type CompositionGate } from "./composerComposition";
import { COMPOSER_MIN_HEIGHT, COMPOSER_MAX_HEIGHT, measureComposerAutosize } from "./composerAutosize";
import { COMPOSER_PANEL_HEIGHT_SHARE, resolveComposerLayout, type ComposerLayout } from "./composerLayoutModel";
import { useComposerHeight } from "./useComposerHeight";

export function useComposerLayout(input: string, textareaRef: RefObject<HTMLTextAreaElement | null>, shellRef: RefObject<HTMLDivElement | null>, gate: CompositionGate, inputVisible = true, expanded = false, active = true) {
  const { keyboardOpen } = useKeyboardInset();
  const [layout, setLayout] = useState<ComposerLayout>({ mode: "docked", editorPx: COMPOSER_MIN_HEIGHT, scrolls: false });
  const previous = useRef(layout);
  useComposerHeight(shellRef);
  const measure = useCallback(() => {
    const textarea = textareaRef.current;
    const shell = shellRef.current;
    if (!textarea || !shell || compositionDefersAutosize(gate)) return;
    if (expanded) {
      const expandedHeight = "min(56dvh, calc(var(--ct-visual-height, 100dvh) - 12rem), 32rem)";
      if (textarea.style.height !== expandedHeight) textarea.style.height = expandedHeight;
      if (textarea.style.maxHeight !== "100%") textarea.style.maxHeight = "100%";
      if (textarea.style.overflowY !== "auto") textarea.style.overflowY = "auto";
      return;
    }
    if (textarea.style.maxHeight !== "") textarea.style.maxHeight = "";
    const panel = shell.closest<HTMLElement>('[data-testid="terminal-tab"], [data-testid="tab-session-chat"]') ?? shell.ownerDocument.documentElement;
    if (!panel) return;
    const bounds = panel.getBoundingClientRect();
    const viewportHeight = window.visualViewport?.height ?? window.innerHeight;
    const panelHeightPx = Math.min(bounds.height || viewportHeight, viewportHeight);
    const maxPx = Math.max(COMPOSER_MIN_HEIGHT, Math.min(COMPOSER_MAX_HEIGHT, panelHeightPx * COMPOSER_PANEL_HEIGHT_SHARE));
    // Skip forced scrollHeight reflow during resize when the composer has no text to wrap.
    const contentPx = textarea.value.length === 0
      ? COMPOSER_MIN_HEIGHT
      : measureComposerAutosize(textarea, shell, maxPx).contentPx;
    const next = resolveComposerLayout({ contentPx, minPx: COMPOSER_MIN_HEIGHT, editorMaxPx: COMPOSER_MAX_HEIGHT,
      panelWidthPx: bounds.width, panelHeightPx, keyboardOpen });
    const nextHeight = `${next.editorPx}px`;
    const nextOverflow = next.scrolls ? "auto" : "hidden";
    if (textarea.style.height !== nextHeight) textarea.style.height = nextHeight;
    if (textarea.style.overflowY !== nextOverflow) textarea.style.overflowY = nextOverflow;
    if (next.mode !== previous.current.mode || next.editorPx !== previous.current.editorPx || next.scrolls !== previous.current.scrolls) {
      previous.current = next;
      setLayout(next);
    }
  }, [gate, keyboardOpen, shellRef, textareaRef, expanded]);
  useLayoutEffect(() => { if (active) measure(); }, [active, input, measure, inputVisible]);
  useLayoutEffect(() => {
    if (!active) return;
    const shell = shellRef.current;
    const textarea = textareaRef.current;
    const panel = shell?.closest<HTMLElement>('[data-testid="terminal-tab"], [data-testid="tab-session-chat"]') ?? shell?.ownerDocument.documentElement;
    if (!panel || !textarea || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(panel);
    observer.observe(textarea);
    window.visualViewport?.addEventListener("resize", measure);
    return () => { observer.disconnect(); window.visualViewport?.removeEventListener("resize", measure); };
  }, [active, measure, shellRef, textareaRef, inputVisible]);
  return { layout, measure };
}
