import type { ButtonHTMLAttributes } from "react";

export interface LiquidToggleProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "onChange"> {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}

export function LiquidToggle({ checked, onCheckedChange, className, ...props }: LiquidToggleProps) {
  return <button {...props} type="button" role="switch" aria-checked={checked} onClick={() => onCheckedChange(!checked)} className={["ct-liquid-toggle", checked ? "ct-liquid-toggle--checked" : "", className ?? ""].filter(Boolean).join(" ")}><span aria-hidden className="ct-liquid-toggle__knob" /></button>;
}
