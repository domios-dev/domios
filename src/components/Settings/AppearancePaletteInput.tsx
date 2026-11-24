import { useCallback, useEffect, useEffectEvent, useLayoutEffect, useRef, useState } from "react";
import { PALETTE_CONTROLS, TERMINAL_COLOURS, type AppearanceLook } from "../../lib/appearance";
import type { ThemeConfig } from "../../lib/types";

type ColourKey = (typeof PALETTE_CONTROLS)[number]["key"] | (typeof TERMINAL_COLOURS)[number];
type ColourPatch = Partial<Pick<ThemeConfig, ColourKey>>;
type DraftUpdate = (look: AppearanceLook | undefined) => AppearanceLook | undefined;

const AUDIT_PAUSE_MS = 200;

export function useAppearancePalette(setDraft: (update: DraftUpdate) => void) {
  const [editing, setEditing] = useState(false);
  const pending = useRef<{ colours: ColourPatch; frame?: number; pause?: number }>({ colours: {} });

  const cancel = useCallback(() => {
    const work = pending.current;
    if (work.frame !== undefined) window.cancelAnimationFrame(work.frame);
    window.clearTimeout(work.pause);
    pending.current = { colours: {} };
  }, []);
  useLayoutEffect(() => cancel, [cancel]);

  const flush = () => {
    const work = pending.current;
    const colours = work.colours;
    work.frame = undefined;
    work.colours = {};
    setDraft(look => look && Object.entries(colours).some(([key, colour]) => look.theme[key as ColourKey] !== colour)
      ? { ...look, theme: { ...look.theme, ...colours } } : look);
    if (work.pause === undefined) setEditing(false);
  };

  const commit = () => {
    const work = pending.current;
    window.clearTimeout(work.pause);
    work.pause = undefined;
    if (work.frame === undefined) setEditing(false);
  };

  const editColour = (key: ColourKey, colour: string) => {
    const work = pending.current;
    work.colours[key] = colour;
    window.clearTimeout(work.pause);
    work.pause = window.setTimeout(commit, AUDIT_PAUSE_MS);
    if (work.frame === undefined) work.frame = window.requestAnimationFrame(flush);
    setEditing(true);
  };

  const editDraft = (update: DraftUpdate) => {
    commit();
    const frame = pending.current.frame;
    if (frame !== undefined) {
      window.cancelAnimationFrame(frame);
      flush();
    }
    setDraft(update);
  };

  const reset = useCallback(() => {
    cancel();
    setEditing(false);
  }, [cancel]);
  return { editing, editColour, editDraft, commit, reset };
}

export function AppearancePaletteInput({ id, label, field, colour, editing, onEdit, onCommit }: {
  id?: string;
  label: string;
  field: string;
  colour: string;
  editing: boolean;
  onEdit(colour: string): void;
  onCommit(): void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const lastInput = useRef(colour);
  const edit = (value: string) => {
    if (lastInput.current === value) return;
    lastInput.current = value;
    onEdit(value);
  };
  const commit = useEffectEvent((value: string) => {
    edit(value);
    onCommit();
  });
  useEffect(() => {
    const element = input.current;
    if (!element) return;
    const onChange = () => commit(element.value);
    element.addEventListener("change", onChange);
    return () => element.removeEventListener("change", onChange);
  }, []);
  useLayoutEffect(() => {
    if (editing || !input.current) return;
    if (input.current.value !== colour) input.current.value = colour;
    lastInput.current = colour;
  }, [colour, editing]);
  return <input ref={input} type="color" id={id} aria-label={label} data-appearance-field={field}
    defaultValue={colour} onInput={event => edit(event.currentTarget.value)} onPointerUp={onCommit} />;
}
