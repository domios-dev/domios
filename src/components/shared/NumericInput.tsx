import { useEffect, useRef, useState, type InputHTMLAttributes, type KeyboardEvent } from "react";
import { formatNumericInput, parseNumericInput, stepNumericValue } from "./numericInputModel";

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
  const [focused, setFocused] = useState(false);
  const lastExternalRef = useRef(value);
  const draftRef = useRef(draft);
  draftRef.current = draft;

  useEffect(() => {
    if (Object.is(value, lastExternalRef.current)) return;
    lastExternalRef.current = value;
    if (!focused) {
      const formatted = formatNumericInput(value);
      draftRef.current = formatted;
      setDraft(formatted);
      return;
    }
    const current = draftRef.current;
    if (value == null && onEmpty && current.trim() === "") return;
    const parsed = parseNumericInput(current, { integer, min, max });
    if (parsed !== null && Object.is(parsed, value)) return;
    const formatted = formatNumericInput(value);
    draftRef.current = formatted;
    setDraft(formatted);
  }, [value, focused, integer, min, max, onEmpty]);

  const parsedDraft = parseNumericInput(draft, { integer, min, max });
  const isInvalid = draft.trim() === "" ? !onEmpty : parsedDraft === null;

  const handleRawChange = (nextRaw: string) => {
    if (nextRaw === draftRef.current) return;
    draftRef.current = nextRaw;
    setDraft(nextRaw);
    if (nextRaw.trim() === "") {
      if (onEmpty && value != null) onEmpty();
      return;
    }
    const parsed = parseNumericInput(nextRaw, { integer, min, max });
    if (parsed !== null && !Object.is(parsed, value)) {
      onValueChange(parsed);
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    onKeyDown?.(event);
    if (event.defaultPrevented) return;
    const key = event.code || event.key;
    if (key !== "ArrowUp" && key !== "ArrowDown") return;
    event.preventDefault();
    const direction = key === "ArrowUp" ? 1 : -1;
    const base = parseNumericInput(draft, { integer }) ?? value;
    const next = stepNumericValue(base, direction, { step, min, max, integer });
    const formatted = formatNumericInput(next);
    draftRef.current = formatted;
    setDraft(formatted);
    if (!Object.is(next, value)) onValueChange(next);
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
        setFocused(true);
        onFocus?.(event);
      }}
      onBlur={(event) => {
        setFocused(false);
        const formatted = formatNumericInput(value);
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
