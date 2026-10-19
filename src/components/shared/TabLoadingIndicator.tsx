import { ProviderGlyph } from "../ProviderGlyph";
import { DeferredLoader } from "./DeferredLoader";
import { BrandMark } from "../brand/BrandMark";

export function TabLoadingIndicator({ label, iconSvg, iconColor, iconHint, compact = false, deferred = true, role = "status", className }: {
  label: string;
  iconSvg?: string | null;
  iconColor?: string | null;
  iconHint?: string | null;
  compact?: boolean;
  deferred?: boolean;
  role?: "status" | "img";
  className?: string;
}) {
  const mark = <span data-testid="tab-loading-mark" role={role} aria-label={label} className={`inline-flex items-center justify-center text-[var(--ct-muted-fg)] ${className ?? ""}`}>
    {!compact && iconSvg
      ? <ProviderGlyph iconSvg={iconSvg} iconColor={iconColor} iconHint={iconHint} className="text-[36px] leading-none" />
      : <BrandMark motion="static" size={compact ? 12 : 22} />}
  </span>;
  return deferred ? <DeferredLoader state="present" className="flex items-center justify-center">{mark}</DeferredLoader> : mark;
}
