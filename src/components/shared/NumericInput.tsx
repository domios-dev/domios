import { useEffect, useRef, useState, type InputHTMLAttributes, type KeyboardEvent } from "react";
import {
  formatNumericInput,
  formatPendingEmission,
  lastCommittedNumeric,
  parseNumericInput,
  reconcileExternalNumericValue,
  stepNumericValue,
  type PendingNumericEmission,
} from "./numericInputModel";

export interface NumericInputProps extends Omit<
  InputHTMLAttributes<HTMLInputElement>,
  "type" | "value" | "defaultValue" | "onChange" | "onInput" | "min" | "max" | "step" | "inputMode"
> {
  value: number | null | undefined;
  onValueChange: (value: number) => void;
  onEmpty?: () => void;
  min?: number;
  max?: number;
  step?: number;
  integer?: boolean;
}

export function NumericInput({
  value,
  onValueChange,
  onEmpty,
  min,
  max,
  step,
  integer = false,
  onFocus,
  onBlur,
  onKeyDown,
  ...rest
}: NumericInputProps) {
  const [draft, setDraft] = useState(() => formatNumericInput(value));
  const focusedRef = useRef(false);
  const lastExternalRef = useRef(value);
  const draftRef = useRef(draft);
  const pendingRef = useRef<PendingNumericEmission[]>([]);
  draftRef.current = draft;

  useEffect(() => {
    if (Object.is(value, lastExternalRef.current)) return;
    lastExternalRef.current = value;
    const reconciled = reconcileExternalNumericValue(pendingRef.current, value, focusedRef.current);
    pendingRef.current = reconciled.pending;
    if (reconciled.nextDraft !== null) {
      draftRef.current = reconciled.nextDraft;
      setDraft(reconciled.nextDraft);
    }
  }, [value]);

  const parsedDraft = parseNumericInput(draft, { integer, min, max });
  const isInvalid = draft.trim() === "" ? !onEmpty : parsedDraft === null;

  const emitValue = (next: number) => {
    const committed = lastCommittedNumeric(pendingRef.current, value);
    if (committed.kind === "value" && Object.is(committed.value, next)) return;
    pendingRef.current = [...pendingRef.current, { kind: "value", value: next }];
    onValueChange(next);
  };

  const emitEmpty = () => {
    if (!onEmpty) return;
    const committed = lastCommittedNumeric(pendingRef.current, value);
    if (committed.kind === "empty") return;
    pendingRef.current = [...pendingRef.current, { kind: "empty" }];
    onEmpty();
  };

  const handleRawChange = (nextRaw: string) => {
    if (nextRaw === draftRef.current) return;
    draftRef.current = nextRaw;
    setDraft(nextRaw);
    if (nextRaw.trim() === "") {
      emitEmpty();
      return;
    }
    const parsed = parseNumericInput(nextRaw, { integer, min, max });
    if (parsed !== null) {
      emitValue(parsed);
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    onKeyDown?.(event);
    if (event.defaultPrevented) return;
    const key = event.code || event.key;
    if (key !== "ArrowUp" && key !== "ArrowDown") return;
    event.preventDefault();
    const direction = key === "ArrowUp" ? 1 : -1;
    const committed = lastCommittedNumeric(pendingRef.current, value);
    const fallback = committed.kind === "value" ? committed.value : undefined;
    const base = parseNumericInput(draftRef.current, { integer }) ?? fallback;
    const next = stepNumericValue(base, direction, { step, min, max, integer });
    const formatted = formatNumericInput(next);
    draftRef.current = formatted;
    setDraft(formatted);
    emitValue(next);
  };

  return (
    <input
      {...rest}
      // Text input avoids Chromium/WebView2 OS-locale comma formatting on <input type="number">.
      type="text"
      inputMode={integer ? "numeric" : "decimal"}
      autoComplete="off"
      spellCheck={false}
      value={draft}
      aria-invalid={rest["aria-invalid"] ?? (isInvalid || undefined)}
      onFocus={(event) => {
        focusedRef.current = true;
        onFocus?.(event);
      }}
      onBlur={(event) => {
        focusedRef.current = false;
        const formatted = formatPendingEmission(lastCommittedNumeric(pendingRef.current, value));
        draftRef.current = formatted;
        setDraft(formatted);
        onBlur?.(event);
      }}
      onKeyDown={handleKeyDown}
      onInput={(event) => handleRawChange(event.currentTarget.value)}
      onChange={(event) => handleRawChange(event.currentTarget.value)}
    />
  );
}
