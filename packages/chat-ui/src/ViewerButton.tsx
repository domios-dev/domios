import { Button, type ButtonProps } from "./Button";

export const VIEWER_ACTION = "ct-liquid-interactive inline-flex shrink-0 items-center justify-center gap-1.5 rounded-md border px-2 text-[11px] hover:bg-[var(--ct-surface)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]";
export const VIEWER_ACTION_STYLE = { minWidth: 44, minHeight: 44, borderColor: "var(--ct-border)", color: "var(--ct-fg)" };

export function ViewerButton({ className, style, ...props }: ButtonProps) {
  return <Button {...props} className={[VIEWER_ACTION, className].filter(Boolean).join(" ")} style={{ ...VIEWER_ACTION_STYLE, ...style }} />;
}
