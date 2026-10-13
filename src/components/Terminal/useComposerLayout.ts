import { useCallback, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { useKeyboardInset } from "../../hooks/useKeyboardInset";
import { compositionDefersAutosize, type CompositionGate } from "./composerComposition";
import { COMPOSER_MIN_HEIGHT, COMPOSER_MAX_HEIGHT, measureComposerAutosize } from "./composerAutosize";
import { resolveComposerLayout, type ComposerLayout } from "./composerLayoutModel";
import { useComposerHeight } from "./useComposerHeight";

export function useComposerLayout(input: string, textareaRef: RefObject<HTMLTextAreaElement | null>, shellRef: RefObject<HTMLDivElement | null>, gate: CompositionGate, inputVisible = true) {
  const { keyboardOpen } = useKeyboardInset();
  const [layout, setLayout] = useState<ComposerLayout>({ mode: "docked", editorPx: COMPOSER_MIN_HEIGHT, scrolls: false });
  const previous = useRef(layout);
  useComposerHeight(shellRef);
  const measure = useCallback(() => {
    const textarea = textareaRef.current;
    const shell = shellRef.current;
    if (!textarea || !shell || compositionDefersAutosize(gate)) return;
    const panel = shell.closest<HTMLElement>('[data-testid="terminal-tab"]') ?? shell.parentElement;
    if (!panel) return;
    const bounds = panel.getBoundingClientRect();
    const viewportHeight = window.visualViewport?.height ?? window.innerHeight;
    const panelHeightPx = Math.min(bounds.height || viewportHeight, viewportHeight);
    const maxPx = Math.max(COMPOSER_MIN_HEIGHT, Math.min(COMPOSER_MAX_HEIGHT, panelHeightPx * 0.4));
    const measured = measureComposerAutosize(textarea, shell, maxPx);
    const next = resolveComposerLayout({ contentPx: measured.contentPx, minPx: COMPOSER_MIN_HEIGHT, editorMaxPx: COMPOSER_MAX_HEIGHT,
      panelWidthPx: bounds.width, panelHeightPx, keyboardOpen });
    textarea.style.height = `${next.editorPx}px`;
    textarea.style.overflowY = next.scrolls ? "auto" : "hidden";
    if (next.mode !== previous.current.mode || next.editorPx !== previous.current.editorPx || next.scrolls !== previous.current.scrolls) {
      previous.current = next;
      setLayout(next);
    }
  }, [gate, keyboardOpen, shellRef, textareaRef]);
  useLayoutEffect(() => { measure(); }, [input, measure, inputVisible]);
  useLayoutEffect(() => {
    const shell = shellRef.current;
    const textarea = textareaRef.current;
    const panel = shell?.closest<HTMLElement>('[data-testid="terminal-tab"]') ?? shell?.parentElement;
    if (!panel || !textarea || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(panel);
    observer.observe(textarea);
    window.visualViewport?.addEventListener("resize", measure);
    return () => { observer.disconnect(); window.visualViewport?.removeEventListener("resize", measure); };
  }, [measure, shellRef, textareaRef, inputVisible]);
  return { layout, measure };
}
