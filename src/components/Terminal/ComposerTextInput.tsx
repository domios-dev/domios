import { forwardRef, type TextareaHTMLAttributes } from "react";

export const ComposerTextInput = forwardRef<HTMLTextAreaElement, TextareaHTMLAttributes<HTMLTextAreaElement>>(
  function ComposerTextInput({ className = "", style, ...props }, ref) {
    return <textarea ref={ref} rows={1} autoComplete="off" autoCorrect="off" autoCapitalize="sentences"
      spellCheck={false} inputMode="text"
      className={`block w-full min-h-5 resize-none bg-transparent py-0 pr-1 text-[14px] leading-5 outline-none placeholder:text-[var(--ct-muted-fg)] ${className}`}
      style={{ color: "var(--ct-fg, #c9d1d9)", caretColor: "var(--ct-fg, #c9d1d9)", border: "none", overflowY: "auto", ...style }}
      {...props} />;
  },
);
